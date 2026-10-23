// SPDX-License-Identifier: GPL-3.0-or-later
//! `index.db`: the rebuildable cache of Locations and Items (ADR-0003). WAL with
//! `synchronous=NORMAL`: losing the last moments of a scan only means rescanning.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kipple_core::category::{TypeCategory, extension_of};
use kipple_core::enrichment::{Enrichment, MediaMeta};
use kipple_core::identity::{ContentHash, ContentStamp, FileId, Identity, VolumeId};
use kipple_core::item::{Item, ItemId, ItemKind};
use kipple_core::location::{
    Location, LocationId, LocationKind, StorageKind, TrashSupport, VolumeCaps,
};
use kipple_core::time::Timestamp;
use rusqlite::{Connection, OptionalExtension, Result, Row, ToSql, params};

use crate::path_bytes;
use crate::schema::{DDL, SCHEMA_VERSION};

/// Where the next page starts: after this (name, id) in name order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub name: String,
    pub id: ItemId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub items: Vec<Item>,
    /// `None` on the last page.
    pub next: Option<Cursor>,
}

#[derive(Debug)]
pub struct IndexDb {
    conn: Connection,
}

/// The columns `read_item` expects, in order (the name last, for the cursor).
const ITEM_COLUMNS: &str = "id, location_id, path, kind, size, created_at, modified_at, changed_at, \
     file_id, volume_id, type_category, is_hidden, is_placeholder, is_screenshot, quick_hash, \
     full_hash, is_os_metadata, name";
/// The position of `name` in [`ITEM_COLUMNS`].
const NAME_COLUMN: usize = 17;

/// The columns written by a scan, in the order of the staging table and the upsert.
const SCAN_COLUMNS: &str = "location_id, path, parent_path, name, ext, kind, size, created_at, \
     modified_at, changed_at, file_id, volume_id, type_category, is_hidden, is_placeholder, \
     is_screenshot, quick_hash, full_hash, is_os_metadata, scanned_at";

/// One page of Items matching `filter` (parameters `?1`…`?n`), in name order, after the cursor
/// `(?n+1, ?n+2)` if `keyset`, then the row limit as the last parameter.
fn page_sql(filter: &str, n: usize, keyset: bool) -> String {
    let (keyset, limit) = if keyset {
        (format!(" AND (name, id) > (?{}, ?{})", n + 1, n + 2), n + 3)
    } else {
        (String::new(), n + 1)
    };
    format!(
        "SELECT {ITEM_COLUMNS} FROM item WHERE {filter}{keyset} ORDER BY name, id LIMIT ?{limit}"
    )
}

fn children_sql(keyset: bool) -> String {
    page_sql("location_id = ?1 AND parent_path = ?2", 2, keyset)
}

/// A Location's Items still to enrich after the ID `?2`, by ID, `?3` at a time. The filter
/// repeats the `item_to_enrich` index's own `WHERE`, word for word, so SQLite picks it.
fn to_enrich_sql() -> String {
    format!(
        "SELECT {ITEM_COLUMNS} FROM item WHERE location_id = ?1 AND id > ?2 AND {TO_ENRICH} \
         ORDER BY id LIMIT ?3"
    )
}

/// Files and bundles (for their download origin) that no enricher has looked at, or that
/// changed since. OS metadata isn't worth reading.
const TO_ENRICH: &str = "enriched_at IS NULL AND kind IN ('file', 'bundle') AND is_os_metadata = 0";

impl IndexDb {
    /// Opens (or creates) the index at `path`. A different schema version means the index is
    /// dropped and recreated empty; the next scan fills it again.
    pub fn open(path: &Path) -> Result<Self> {
        // 64 MiB page cache: the item indexes and the trigram index outgrow the 2 MiB default
        // quickly, and every cache miss is a disk read when browsing and searching.
        Self::open_with_cache(path, 64)
    }

    /// A connection for one scan: scans run side by side with the app's own connection, so
    /// each gets a smaller page cache (a scan touches one folder's rows at a time).
    pub fn open_for_scan(path: &Path) -> Result<Self> {
        Self::open_with_cache(path, 16)
    }

    fn open_with_cache(path: &Path, cache_mib: i64) -> Result<Self> {
        let conn = Connection::open(path)?;
        // Scans write through their own connections while the app reads and writes locations:
        // wait for the others' transactions (first, so even the pragmas below wait).
        conn.busy_timeout(Duration::from_secs(30))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "cache_size", -cache_mib * 1024)?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        Self::init(conn)
    }

    /// Like [`IndexDb::open`], but a damaged index never stops Kipple: the file (and its WAL
    /// files) is moved aside as `index.db.broken…` and a fresh one is started; if even that
    /// fails, an in-memory index. The index is a cache, so a rescan refills it. Also returns why
    /// the old one was dropped, for the log.
    pub fn open_or_recreate(path: &Path) -> (Self, Option<String>) {
        let reason = match Self::open(path) {
            Ok(db) => return (db, None),
            Err(e) => e.to_string(),
        };
        move_aside(path);
        match Self::open(path).or_else(|_| Self::open_in_memory()) {
            Ok(db) => (db, Some(reason)),
            // An in-memory database with the schema can't fail short of running out of memory.
            Err(e) => panic!("can't even create an in-memory index: {e}"),
        }
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            // Off while dropping: otherwise dropping `location` cascades into `item`, row by row,
            // through triggers whose search table may already be gone. (Ignored inside a
            // transaction, so set before it.)
            conn.pragma_update(None, "foreign_keys", false)?;
            let tx = conn.transaction()?;
            // Dropping a table also drops its indexes and triggers, and an FTS table its shadow
            // tables, so ask again after each drop.
            while let Some((kind, name)) = tx
                .query_row(
                    "SELECT type, name FROM sqlite_master \
                     WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
                     ORDER BY sql LIKE 'CREATE VIRTUAL%' DESC LIMIT 1",
                    [],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?
            {
                tx.execute_batch(&format!("DROP {kind} \"{}\"", name.replace('"', "\"\"")))?;
            }
            tx.execute_batch(DDL)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        conn.pragma_update(None, "foreign_keys", true)?;
        // Scan batches are staged here, then upserted in one statement: one search-index flush
        // per batch instead of one per row.
        conn.execute_batch(&format!(
            "CREATE TEMP TABLE IF NOT EXISTS item_stage AS SELECT {SCAN_COLUMNS} FROM item WHERE 0"
        ))?;
        Ok(Self { conn })
    }

    /// Adds or replaces a Location (by its ID). Never deletes it, so its Items stay.
    pub fn put_location(&mut self, location: &Location) -> Result<()> {
        self.conn.execute(
            "INSERT INTO location (id, path, kind, enabled, volume_id, online, last_seen_online_at, \
               cap_trash, cap_case_sensitive, cap_stable_ids, cap_storage, in_sync_root) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
             ON CONFLICT (id) DO UPDATE SET path = excluded.path, kind = excluded.kind, \
               enabled = excluded.enabled, volume_id = excluded.volume_id, online = excluded.online, \
               last_seen_online_at = excluded.last_seen_online_at, cap_trash = excluded.cap_trash, \
               cap_case_sensitive = excluded.cap_case_sensitive, \
               cap_stable_ids = excluded.cap_stable_ids, cap_storage = excluded.cap_storage, \
               in_sync_root = excluded.in_sync_root",
            params![
                location.id.0,
                path_bytes::encode(&location.path),
                location.kind.code(),
                location.enabled,
                location.volume.0,
                location.online,
                location.last_seen_online.map(|t| t.0),
                location.caps.trash.code(),
                location.caps.case_sensitive,
                location.caps.stable_file_ids,
                location.caps.storage.code(),
                location.in_sync_root,
            ],
        )?;
        Ok(())
    }

    /// Every Location in the index, by ID.
    pub fn locations(&self) -> Result<Vec<Location>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, path, kind, enabled, volume_id, online, last_seen_online_at, cap_trash, \
               cap_case_sensitive, cap_stable_ids, cap_storage, in_sync_root FROM location ORDER BY id",
        )?;
        stmt.query_map([], |r| {
            Ok(Location {
                id: LocationId(r.get(0)?),
                path: path_bytes::decode(&r.get::<_, Vec<u8>>(1)?),
                kind: code(r, 2, LocationKind::from_code)?,
                enabled: r.get(3)?,
                volume: VolumeId(r.get(4)?),
                online: r.get(5)?,
                last_seen_online: r.get::<_, Option<i64>>(6)?.map(Timestamp),
                caps: VolumeCaps {
                    trash: code(r, 7, TrashSupport::from_code)?,
                    case_sensitive: r.get(8)?,
                    stable_file_ids: r.get(9)?,
                    storage: code(r, 10, StorageKind::from_code)?,
                },
                in_sync_root: r.get(11)?,
            })
        })?
        .collect()
    }

    /// Removes a Location and, with it, all its Items.
    pub fn delete_location(&mut self, id: LocationId) -> Result<()> {
        // Items (and their candidates) cascade; the search triggers clean up the name index.
        self.conn
            .execute("DELETE FROM location WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// Inserts or updates Items by (location, path) in one transaction, returning their IDs in
    /// order. An update keeps the row ID, the last-used signal, the download origin and the
    /// screenshot flag: the walker knows none of these, so it can set the flag but never clears
    /// it ([`IndexDb::apply_enrichment`] does). The origin is kept only while the file ID is the
    /// same (or both have none, as on FAT): another file saved at the path loses it, and so does
    /// one rewritten by an atomic save (a new file ID), since a wrong origin is worse than a
    /// missing one, which the enrichers may find again. What was read from the content (hashes,
    /// mime, EXIF, a sniffed category, `enriched_at`) is kept only while the file ID, size, mtime
    /// and ctime are unchanged: otherwise it's cleared, which tells the enrichers to look again.
    /// So is `enriched_at` when a placeholder is downloaded: its content can be read now. Until
    /// then the category is the walker's, still a screenshot if it's an image.
    pub fn upsert_items(&mut self, items: &[Item], scanned_at: Timestamp) -> Result<Vec<ItemId>> {
        self.upsert(items, scanned_at, true)
    }

    /// [`IndexDb::upsert_items`] without returning the IDs: what a scan writes.
    pub fn upsert_scanned(&mut self, items: &[Item], scanned_at: Timestamp) -> Result<()> {
        self.upsert(items, scanned_at, false).map(drop)
    }

    fn upsert(&mut self, items: &[Item], scanned_at: Timestamp, ids: bool) -> Result<Vec<ItemId>> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM item_stage", [])?;
        {
            let mut stage = tx.prepare_cached(&format!(
                "INSERT INTO item_stage ({SCAN_COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
                   ?18, ?19, ?20)"
            ))?;
            for item in items {
                let path = &normal(&item.identity.path);
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                let (quick, full) = match item.content.hash {
                    Some(ContentHash::Quick(h)) => (Some(h), None),
                    Some(ContentHash::Full(h)) => (None, Some(h)),
                    None => (None, None),
                };
                stage.execute(params![
                    item.location.0,
                    path_bytes::encode(path),
                    path_bytes::encode(path.parent().unwrap_or(Path::new(""))),
                    name,
                    extension_of(&name),
                    item.kind.code(),
                    i64::try_from(item.content.size).unwrap_or(i64::MAX),
                    item.created.map(|t| t.0),
                    item.content.modified.0,
                    item.content.changed.map(|t| t.0),
                    item.identity.file.map(|f| f.0.to_be_bytes()),
                    item.identity.volume.0,
                    item.category.code(),
                    item.is_hidden,
                    item.is_placeholder,
                    item.is_screenshot,
                    quick,
                    full,
                    item.is_os_metadata,
                    scanned_at.0,
                ])?;
            }
        }
        // `WHERE true` keeps SQLite from reading `ON CONFLICT` as part of the SELECT.
        let unchanged = "item.file_id IS excluded.file_id AND item.size = excluded.size \
            AND item.modified_at = excluded.modified_at AND item.changed_at IS excluded.changed_at";
        let enriched = format!(
            "{unchanged} AND NOT (item.is_placeholder AND NOT excluded.is_placeholder) \
             AND item.enriched_at IS NOT NULL"
        );
        let (images, screenshots) = (
            TypeCategory::Images.code(),
            TypeCategory::Screenshots.code(),
        );
        let returning = if ids {
            "RETURNING location_id, path, id"
        } else {
            ""
        };
        let mut ids_by_key = std::collections::HashMap::with_capacity(items.len());
        {
            let mut upsert = tx.prepare_cached(&format!(
                "INSERT INTO item ({SCAN_COLUMNS}) SELECT {SCAN_COLUMNS} FROM item_stage WHERE true \
                 ON CONFLICT (location_id, path) DO UPDATE SET parent_path = excluded.parent_path, \
                   name = excluded.name, ext = excluded.ext, kind = excluded.kind, \
                   size = excluded.size, created_at = excluded.created_at, \
                   modified_at = excluded.modified_at, changed_at = excluded.changed_at, \
                   file_id = excluded.file_id, volume_id = excluded.volume_id, \
                   type_category = iif({enriched}, item.type_category, \
                     iif(item.is_screenshot AND excluded.type_category = '{images}', \
                       '{screenshots}', excluded.type_category)), \
                   is_screenshot = item.is_screenshot OR excluded.is_screenshot, \
                   is_hidden = excluded.is_hidden, is_placeholder = excluded.is_placeholder, \
                   is_os_metadata = excluded.is_os_metadata, \
                   origin_url = iif(item.file_id IS excluded.file_id, item.origin_url, NULL), \
                   quick_hash = coalesce(excluded.quick_hash, iif({unchanged}, item.quick_hash, NULL)), \
                   full_hash = coalesce(excluded.full_hash, iif({unchanged}, item.full_hash, NULL)), \
                   hashed_at = iif({unchanged}, item.hashed_at, NULL), \
                   phash = iif({unchanged}, item.phash, NULL), \
                   mime = iif({unchanged}, item.mime, NULL), \
                   meta_json = iif({unchanged}, item.meta_json, NULL), \
                   enriched_at = iif({enriched}, item.enriched_at, NULL), \
                   scanned_at = excluded.scanned_at {returning}"
            ))?;
            // RETURNING has no guaranteed order: map the IDs back by (location, path).
            let mut rows = upsert.query([])?;
            while let Some(row) = rows.next()? {
                ids_by_key.insert(
                    (row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?),
                    ItemId(row.get(2)?),
                );
            }
        }
        tx.commit()?;
        if !ids {
            return Ok(Vec::new());
        }
        Ok(items
            .iter()
            .map(|item| {
                ids_by_key[&(
                    item.location.0,
                    path_bytes::encode(&normal(&item.identity.path)),
                )]
            })
            .collect())
    }

    pub fn item(&self, id: ItemId) -> Result<Option<Item>> {
        self.conn
            .query_row(
                &format!("SELECT {ITEM_COLUMNS} FROM item WHERE id = ?1"),
                [id.0],
                read_item,
            )
            .optional()
    }

    /// The Items directly inside `parent`, sorted by name (case-insensitive), `limit` at a time.
    pub fn children(
        &self,
        location: LocationId,
        parent: &Path,
        after: Option<&Cursor>,
        limit: usize,
    ) -> Result<Page> {
        let parent = path_bytes::encode(&normal(parent));
        self.page(
            &children_sql(after.is_some()),
            &[&location.0, &parent],
            after,
            limit,
        )
    }

    /// Every Item directly inside `parent`, in no particular order: what a rescan compares a
    /// folder's listing with.
    pub fn items_in(&self, location: LocationId, parent: &Path) -> Result<Vec<Item>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ITEM_COLUMNS} FROM item WHERE location_id = ?1 AND parent_path = ?2"
        ))?;
        stmt.query_map(
            params![location.0, path_bytes::encode(&normal(parent))],
            read_item,
        )?
        .collect()
    }

    /// Deletes these Items (not what's inside them: see [`IndexDb::delete_under`]).
    /// Returns how many there were.
    pub fn delete_items(&mut self, ids: &[ItemId]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut deleted = 0;
        {
            let mut delete = tx.prepare_cached("DELETE FROM item WHERE id = ?1")?;
            for id in ids {
                deleted += delete.execute([id.0])?;
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// Deletes every Item inside the folder `dir` of `location`, at any depth (not `dir`'s own
    /// row). Returns how many there were.
    pub fn delete_under(&mut self, location: LocationId, dir: &Path) -> Result<usize> {
        // Everything inside starts with `dir` + separator: one range of the (location, path)
        // index, from that prefix up to the prefix with its last byte raised by one.
        let mut prefix = path_bytes::encode(&normal(dir).join("x"));
        prefix.truncate(prefix.len() - path_bytes::encode(Path::new("x")).len());
        let mut end = prefix.clone();
        *end.last_mut().expect("a separator") += 1;
        // In chunks: one huge delete would keep other writers (other scans) waiting.
        let mut delete = self.conn.prepare_cached(
            "DELETE FROM item WHERE id IN (SELECT id FROM item \
               WHERE location_id = ?1 AND path >= ?2 AND path < ?3 LIMIT 10000)",
        )?;
        let mut deleted = 0;
        loop {
            match delete.execute(params![location.0, prefix, end])? {
                0 => return Ok(deleted),
                n => deleted += n,
            }
        }
    }

    /// Up to `limit` files and bundles of `location` that no enricher has looked at since they
    /// last changed, by ID, after `after`. Placeholders are included: their metadata (origin,
    /// screenshot flag) can be read, just not their content.
    pub fn items_to_enrich(
        &self,
        location: LocationId,
        after: Option<ItemId>,
        limit: usize,
    ) -> Result<Vec<Item>> {
        let mut stmt = self.conn.prepare_cached(&to_enrich_sql())?;
        stmt.query_map(
            params![
                location.0,
                after.map_or(i64::MIN, |id| id.0),
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            read_item,
        )?
        .collect()
    }

    /// Stores what the enrichers learned, in one transaction, and marks those Items enriched at
    /// `at`. An Item that's gone, or whose file ID, content stamp or placeholder flag changed
    /// since the enricher saw it (a rescan got there first), is skipped: it will be enriched
    /// again. A known origin is kept when none is found this time: a change or a save that
    /// rewrote the file may have dropped where it came from, not changed it. Returns how many
    /// were stored.
    pub fn apply_enrichment(&mut self, enrichments: &[Enrichment], at: Timestamp) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut applied = 0;
        {
            let mut update = tx.prepare_cached(
                "UPDATE item SET mime = ?1, type_category = ?2, is_screenshot = ?3, \
                   origin_url = coalesce(?4, origin_url), meta_json = ?5, enriched_at = ?6 \
                 WHERE id = ?7 AND file_id IS ?8 AND size = ?9 AND modified_at = ?10 \
                   AND changed_at IS ?11 AND is_placeholder = ?12",
            )?;
            for e in enrichments {
                applied += update.execute(params![
                    e.mime,
                    e.category.code(),
                    e.is_screenshot,
                    e.origin_url,
                    meta_json(&e.media),
                    at.0,
                    e.item.0,
                    e.file.map(|f| f.0.to_be_bytes()),
                    i64::try_from(e.content.size).unwrap_or(i64::MAX),
                    e.content.modified.0,
                    e.content.changed.map(|t| t.0),
                    e.is_placeholder,
                ])?;
            }
        }
        tx.commit()?;
        Ok(applied)
    }

    /// Items whose name contains `query` (case- and accent-insensitive), sorted by name.
    pub fn search(&self, query: &str, after: Option<&Cursor>, limit: usize) -> Result<Page> {
        if query.is_empty() {
            return Ok(Page {
                items: Vec::new(),
                next: None,
            });
        }
        // Trigrams need 3 characters; shorter queries scan the names instead.
        if query.chars().count() >= 3 {
            let phrase = format!("\"{}\"", query.replace('"', "\"\""));
            let sql = page_sql(
                "id IN (SELECT rowid FROM item_name_fts WHERE item_name_fts MATCH ?1)",
                1,
                after.is_some(),
            );
            self.page(&sql, &[&phrase], after, limit)
        } else {
            let pattern = format!(
                "%{}%",
                query
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            let sql = page_sql("name LIKE ?1 ESCAPE '\\'", 1, after.is_some());
            self.page(&sql, &[&pattern], after, limit)
        }
    }

    /// Runs a [`page_sql`] query: one row more than asked tells whether there's a next page.
    fn page(
        &self,
        sql: &str,
        filter: &[&dyn ToSql],
        after: Option<&Cursor>,
        limit: usize,
    ) -> Result<Page> {
        let limit = limit.max(1);
        let fetch = i64::try_from(limit).unwrap_or(i64::MAX - 1) + 1;
        let mut params: Vec<&dyn ToSql> = filter.to_vec();
        if let Some(cursor) = after {
            params.push(&cursor.name);
            params.push(&cursor.id.0);
        }
        params.push(&fetch);
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = stmt
            .query_map(params.as_slice(), |r| {
                Ok((read_item(r)?, r.get::<_, String>(NAME_COLUMN)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        let next = if rows.len() > limit {
            rows.truncate(limit);
            rows.last().map(|(item, name)| Cursor {
                name: name.clone(),
                id: item.id.expect("stored"),
            })
        } else {
            None
        };
        Ok(Page {
            items: rows.into_iter().map(|(item, _)| item).collect(),
            next,
        })
    }
}

/// Moves a damaged `index.db` and its WAL files aside (`index.db.broken-<n>`), keeping them for a
/// bug report instead of deleting them.
// Kipple's own cache file, not a user file: ADR-0002's Executor rule covers managed locations.
#[allow(clippy::disallowed_methods)]
fn move_aside(path: &Path) {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let free = (1..)
        .find(|n| !path.with_file_name(format!("{name}.broken-{n}")).exists())
        .unwrap_or(1);
    for suffix in ["", "-wal", "-shm"] {
        let from = path.with_file_name(format!("{name}{suffix}"));
        if from.exists() {
            let _ = std::fs::rename(
                &from,
                path.with_file_name(format!("{name}.broken-{free}{suffix}")),
            );
        }
    }
}

/// Column `idx` read as a stable code.
fn code<T>(r: &Row, idx: usize, from_code: fn(&str) -> Option<T>) -> Result<T> {
    let text: String = r.get(idx)?;
    from_code(&text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Text,
            format!("unknown code {text:?}").into(),
        )
    })
}

/// `path` without `.` components and trailing separators: `/r/`, `/r/.` and `/r` are one row.
fn normal(path: &Path) -> PathBuf {
    path.components().collect()
}

/// `media` as the `meta_json` column: a JSON object of the fields that are known, times in
/// nanoseconds like every other column. `None` when nothing is.
fn meta_json(media: &MediaMeta) -> Option<String> {
    let MediaMeta {
        width,
        height,
        taken_at,
        camera_make,
        camera_model,
        orientation,
    } = media;
    let quoted = |text: &Option<String>| {
        text.as_ref().map(|text| {
            let mut out = String::from('"');
            for c in text.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    c if c < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        })
    };
    let fields = [
        ("width", width.map(|n| n.to_string())),
        ("height", height.map(|n| n.to_string())),
        ("taken_at", taken_at.map(|t| t.0.to_string())),
        ("camera_make", quoted(camera_make)),
        ("camera_model", quoted(camera_model)),
        ("orientation", orientation.map(|n| n.to_string())),
    ];
    let known: Vec<_> = fields
        .into_iter()
        .filter_map(|(key, value)| Some(format!("\"{key}\":{}", value?)))
        .collect();
    (!known.is_empty()).then(|| format!("{{{}}}", known.join(",")))
}

/// An Item from a row of [`ITEM_COLUMNS`]. An unknown kind or category code is an error: only
/// this code writes them, so it means a bug or a damaged index (rescanning rebuilds it).
fn read_item(r: &Row) -> Result<Item> {
    let quick: Option<[u8; 32]> = r.get(14)?;
    let full: Option<[u8; 32]> = r.get(15)?;
    let file_id: Option<[u8; 16]> = r.get(8)?;
    Ok(Item {
        id: Some(ItemId(r.get(0)?)),
        location: LocationId(r.get(1)?),
        kind: code(r, 3, ItemKind::from_code)?,
        identity: Identity {
            volume: VolumeId(r.get(9)?),
            file: file_id.map(|b| FileId(u128::from_be_bytes(b))),
            path: path_bytes::decode(&r.get::<_, Vec<u8>>(2)?),
        },
        content: ContentStamp {
            size: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
            modified: Timestamp(r.get(6)?),
            changed: r.get::<_, Option<i64>>(7)?.map(Timestamp),
            hash: full
                .map(ContentHash::Full)
                .or(quick.map(ContentHash::Quick)),
        },
        created: r.get::<_, Option<i64>>(5)?.map(Timestamp),
        category: code(r, 10, TypeCategory::from_code)?,
        is_hidden: r.get(11)?,
        is_placeholder: r.get(12)?,
        is_screenshot: r.get(13)?,
        is_os_metadata: r.get(16)?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kipple_core::category::TypeCategory;
    use kipple_core::enrichment::{Enrichment, MediaMeta};
    use kipple_core::identity::{ContentHash, ContentStamp, FileId, Identity, VolumeId};
    use kipple_core::item::ItemKind;
    use kipple_core::location::{LocationKind, StorageKind, TrashSupport, UserFolder, VolumeCaps};

    use super::*;

    const ROOT: &str = "/Users/zoe/Downloads";

    fn location() -> Location {
        Location {
            id: LocationId(1),
            path: ROOT.into(),
            kind: LocationKind::UserFolder(UserFolder::Downloads),
            enabled: true,
            volume: VolumeId("vol-1".into()),
            online: true,
            last_seen_online: Some(Timestamp::from_secs(1_700_000_000)),
            caps: VolumeCaps {
                trash: TrashSupport::Yes,
                case_sensitive: false,
                stable_file_ids: true,
                storage: StorageKind::Ssd,
            },
            in_sync_root: false,
        }
    }

    fn item(rel: &str, size: u64) -> Item {
        let path = PathBuf::from(ROOT).join(rel);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        Item {
            id: None,
            location: LocationId(1),
            kind: ItemKind::File,
            identity: Identity {
                volume: VolumeId("vol-1".into()),
                file: Some(FileId(u128::MAX - size as u128)),
                path,
            },
            content: ContentStamp {
                size,
                modified: Timestamp(1_700_000_000_123_456_789),
                changed: Some(Timestamp(1_700_000_001_000_000_000)),
                hash: None,
            },
            created: None,
            category: TypeCategory::of(ItemKind::File, &name, false),
            is_hidden: false,
            is_placeholder: false,
            is_screenshot: false,
            is_os_metadata: false,
        }
    }

    fn db_with(items: &[Item]) -> IndexDb {
        let mut db = IndexDb::open_in_memory().unwrap();
        db.put_location(&location()).unwrap();
        db.upsert_items(items, Timestamp(1)).unwrap();
        db
    }

    fn names(page: &Page) -> Vec<String> {
        page.items
            .iter()
            .map(|i| {
                i.identity
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn locations_round_trip_and_deleting_one_removes_its_items() {
        let mut db = db_with(&[item("a.pdf", 1)]);
        let mut usb = location();
        usb.id = LocationId(2);
        usb.path = "/Volumes/USB".into();
        usb.kind = LocationKind::ExternalDrive;
        usb.online = false;
        usb.last_seen_online = None;
        usb.caps.trash = TrashSupport::No;
        usb.caps.storage = StorageKind::Removable;
        usb.in_sync_root = true;
        db.put_location(&usb).unwrap();
        assert_eq!(db.locations().unwrap(), vec![location(), usb.clone()]);

        db.delete_location(LocationId(1)).unwrap();
        assert_eq!(db.locations().unwrap(), vec![usb]);
        assert!(
            db.search("a.pdf", None, 10).unwrap().items.is_empty(),
            "its items went with it"
        );
    }

    #[test]
    fn items_round_trip_with_every_field() {
        let mut original = item("Setup.dmg", 128_000_000);
        original.kind = ItemKind::Bundle;
        original.created = Some(Timestamp(-5)); // before 1970 is fine
        original.content.hash = Some(ContentHash::Full([7; 32]));
        original.is_hidden = true;
        original.is_placeholder = true;
        original.is_screenshot = true;
        original.is_os_metadata = true;
        let db = db_with(&[]);
        let mut db = db;
        let ids = db
            .upsert_items(std::slice::from_ref(&original), Timestamp(1))
            .unwrap();
        let stored = db.item(ids[0]).unwrap().expect("stored");
        assert_eq!(
            stored,
            Item {
                id: Some(ids[0]),
                ..original
            }
        );
        assert_eq!(db.item(ItemId(999)).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn paths_that_are_not_utf8_survive() {
        use std::os::unix::ffi::OsStrExt;
        let mut odd = item("x", 1);
        odd.identity.path = PathBuf::from(std::ffi::OsStr::from_bytes(
            b"/Users/zoe/Downloads/caf\xe9.txt",
        ));
        let mut db = db_with(&[]);
        let id = db
            .upsert_items(std::slice::from_ref(&odd), Timestamp(1))
            .unwrap()[0];
        assert_eq!(
            db.item(id).unwrap().unwrap().identity.path,
            odd.identity.path
        );
    }

    #[test]
    fn an_upsert_keeps_the_id_and_what_enrichers_added() {
        let mut db = db_with(&[item("a.pdf", 10)]);
        let id = db.upsert_items(&[item("a.pdf", 10)], Timestamp(2)).unwrap()[0];
        db.conn
            .execute(
                "UPDATE item SET mime = 'application/pdf', last_used_at = 42 WHERE id = ?1",
                [id.0],
            )
            .unwrap();
        let again = db.upsert_items(&[item("a.pdf", 99)], Timestamp(3)).unwrap()[0];
        assert_eq!(again, id);
        assert_eq!(db.item(id).unwrap().unwrap().content.size, 99);
        let (mime, last_used): (Option<String>, i64) = db
            .conn
            .query_row(
                "SELECT mime, last_used_at FROM item WHERE id = ?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        // The content changed: its type is sniffed again; when it was last used isn't content.
        assert_eq!((mime, last_used), (None, 42));
    }

    #[test]
    fn children_are_sorted_by_name_and_paged_by_keyset() {
        let db = db_with(&[
            item("b.pdf", 1),
            item("A.pdf", 1),
            item("c.pdf", 1),
            item("sub/inner.pdf", 1),
            item("D.pdf", 1),
        ]);
        let first = db
            .children(LocationId(1), Path::new(ROOT), None, 2)
            .unwrap();
        assert_eq!(names(&first), ["A.pdf", "b.pdf"]);
        let second = db
            .children(LocationId(1), Path::new(ROOT), first.next.as_ref(), 2)
            .unwrap();
        assert_eq!(names(&second), ["c.pdf", "D.pdf"]);
        assert!(
            second.next.is_none(),
            "the last page says so, even when it's full"
        );
        let sub = db
            .children(LocationId(1), &Path::new(ROOT).join("sub"), None, 10)
            .unwrap();
        assert_eq!(names(&sub), ["inner.pdf"]);
        assert!(sub.next.is_none(), "a short page is the last");
    }

    #[test]
    fn search_finds_substrings_ignoring_case_and_accents() {
        let db = db_with(&[
            item("Belastingaangifte 2025.pdf", 1),
            item("Café menu.pdf", 1),
            item("holiday.jpg", 1),
            item("HOLIDAY copy.jpg", 1),
            item("notes.txt", 1),
        ]);
        let find = |q: &str| names(&db.search(q, None, 10).unwrap());
        assert_eq!(find("aangifte"), ["Belastingaangifte 2025.pdf"]);
        assert_eq!(
            find("holiday"),
            ["HOLIDAY copy.jpg", "holiday.jpg"],
            "space sorts before dot"
        );
        assert_eq!(find("cafe"), ["Café menu.pdf"], "accent-insensitive");
        assert_eq!(
            find("no"),
            ["notes.txt"],
            "queries under 3 characters work too"
        );
        assert_eq!(
            find("\"quote"),
            Vec::<String>::new(),
            "quotes in the query are just text"
        );
        assert!(find("zzz").is_empty());
    }

    #[test]
    fn search_pages_by_keyset() {
        let items: Vec<_> = (0..25)
            .map(|i| item(&format!("report {i:02}.pdf"), i))
            .collect();
        let db = db_with(&items);
        let mut seen = Vec::new();
        let mut cursor = None;
        loop {
            let page = db.search("report", cursor.as_ref(), 10).unwrap();
            seen.extend(names(&page));
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        let expected: Vec<_> = (0..25).map(|i| format!("report {i:02}.pdf")).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn an_unknown_code_is_an_error_not_a_guess() {
        let mut db = db_with(&[]);
        let id = db.upsert_items(&[item("a.pdf", 1)], Timestamp(1)).unwrap()[0];
        db.conn
            .execute("UPDATE item SET kind = 'folder' WHERE id = ?1", [id.0])
            .unwrap();
        assert!(db.item(id).is_err());
    }

    #[test]
    fn hashes_are_dropped_when_the_content_changed() {
        let mut hashed = item("a.iso", 10);
        hashed.content.hash = Some(ContentHash::Full([1; 32]));
        let mut db = db_with(&[hashed.clone()]);
        // Rescanned without a hash, content unchanged: the stored hash stays.
        let id = db.upsert_items(&[item("a.iso", 10)], Timestamp(2)).unwrap()[0];
        assert_eq!(
            db.item(id).unwrap().unwrap().content.hash,
            hashed.content.hash
        );
        // What enrichers added, by hand.
        let enrich = |db: &IndexDb| {
            db.conn
                .execute(
                    "UPDATE item SET hashed_at = 1, phash = x'01', mime = 'x/y', meta_json = '{}', \
                       enriched_at = 1, last_used_at = 5, origin_url = 'https://example.org'",
                    [],
                )
                .unwrap();
        };
        // (how many content-derived columns are set, last used, origin)
        let derived = |db: &IndexDb| -> (i64, Option<i64>, Option<String>) {
            db.conn
                .query_row(
                    "SELECT (hashed_at IS NOT NULL) + (phash IS NOT NULL) + (mime IS NOT NULL) \
                       + (meta_json IS NOT NULL) + (enriched_at IS NOT NULL), last_used_at, \
                       origin_url FROM item",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap()
        };
        enrich(&db);
        db.upsert_items(&[item("a.iso", 10)], Timestamp(3)).unwrap();
        assert_eq!(derived(&db).0, 5, "unchanged: the enrichment stays");
        // Rescanned with a new size: the old hash no longer describes the file.
        let mut grown = item("a.iso", 11);
        grown.identity.file = hashed.identity.file;
        db.upsert_items(&[grown], Timestamp(3)).unwrap();
        assert_eq!(db.item(id).unwrap().unwrap().content.hash, None);
        assert_eq!(
            derived(&db),
            (0, Some(5), Some("https://example.org".into())),
            "what came from the content is cleared (enrich again); last used and origin stay"
        );
    }

    #[test]
    fn a_populated_index_is_rebuilt_too() {
        // With foreign keys on, dropping `location` cascades into `item` and its search triggers.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        let mut db = IndexDb::open(&path).unwrap();
        db.put_location(&location()).unwrap();
        db.upsert_items(&[item("a.pdf", 1), item("b.pdf", 2)], Timestamp(1))
            .unwrap();
        db.conn.pragma_update(None, "user_version", 0).unwrap();
        drop(db);
        let db = IndexDb::open(&path).unwrap();
        assert!(
            db.search("pdf", None, 10).unwrap().items.is_empty(),
            "rebuilt empty"
        );
    }

    #[test]
    fn a_rename_through_an_upsert_updates_search() {
        let db = db_with(&[item("old name.pdf", 1)]);
        let id = db.search("old name", None, 10).unwrap().items[0]
            .id
            .unwrap();
        // Same row (location + path), new name: e.g. a case-only rename on a case-insensitive
        // volume, or a name decoded differently. Rewrite the stored name directly.
        db.conn
            .execute(
                "UPDATE item SET name = 'new name.pdf' WHERE id = ?1",
                [id.0],
            )
            .unwrap();
        assert!(db.search("old name", None, 10).unwrap().items.is_empty());
        assert_eq!(
            names(&db.search("new name", None, 10).unwrap()),
            ["old name.pdf"]
        );
    }

    #[test]
    fn like_wildcards_in_short_queries_are_literal() {
        let db = db_with(&[item("50%.txt", 1), item("a_b.txt", 1), item("abc.txt", 1)]);
        assert_eq!(names(&db.search("%", None, 10).unwrap()), ["50%.txt"]);
        assert_eq!(names(&db.search("_", None, 10).unwrap()), ["a_b.txt"]);
    }

    #[test]
    fn later_pages_seek_instead_of_scanning() {
        let db = db_with(&[]);
        let mut stmt = db
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {}", children_sql(true)))
            .unwrap();
        let plan = stmt
            .query_map(params![1, b"/r".to_vec(), "m", 1, 10], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            plan.contains("name>?") || plan.contains("(name,id)>(?,?)"),
            "{plan}"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn a_damaged_index_is_moved_aside_and_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        std::fs::write(&path, b"this is not a database, it's a shopping list").unwrap();
        assert!(IndexDb::open(&path).is_err());
        let (mut db, reason) = IndexDb::open_or_recreate(&path);
        assert!(reason.is_some());
        db.put_location(&location()).unwrap();
        db.upsert_items(&[item("a.pdf", 1)], Timestamp(1)).unwrap();
        assert!(path.exists(), "a fresh file, not in memory");
        let moved: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("index.db.broken"))
            .collect();
        assert_eq!(moved.len(), 1, "{moved:?}");
        // A healthy index opens as is.
        drop(db);
        let (db, reason) = IndexDb::open_or_recreate(&path);
        assert!(reason.is_none());
        assert_eq!(db.locations().unwrap().len(), 1);
    }

    #[test]
    fn a_new_schema_version_rebuilds_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch("CREATE TABLE item (stale TEXT); PRAGMA user_version = 0;")
                .unwrap();
        }
        let mut db = IndexDb::open(&path).unwrap();
        let version: i32 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::SCHEMA_VERSION);
        db.put_location(&location()).unwrap();
        db.upsert_items(&[item("a.pdf", 1)], Timestamp(1)).unwrap();
        drop(db);
        // Reopening the current version keeps the data.
        let db = IndexDb::open(&path).unwrap();
        assert_eq!(names(&db.search("a.pdf", None, 10).unwrap()), ["a.pdf"]);
    }

    /// The items' paths with `/` separators: Windows stores `ROOT` as `\Users\zoe\Downloads`.
    fn paths(items: &[Item]) -> Vec<String> {
        let mut paths: Vec<_> = items
            .iter()
            .map(|i| i.identity.path.to_string_lossy().replace('\\', "/"))
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn a_folders_items_are_read_in_one_go() {
        let db = db_with(&[
            item("a.pdf", 1),
            item("Sub", 0),
            item("Sub/b.pdf", 2),
            item("Sub/Deeper/c.pdf", 3),
        ]);
        let top = db.items_in(LocationId(1), Path::new(ROOT)).unwrap();
        assert_eq!(
            paths(&top),
            [format!("{ROOT}/Sub"), format!("{ROOT}/a.pdf")]
        );
        assert!(top.iter().all(|i| i.id.is_some()));
        let sub = db
            .items_in(LocationId(1), &Path::new(ROOT).join("Sub"))
            .unwrap();
        assert_eq!(paths(&sub), [format!("{ROOT}/Sub/b.pdf")]);
        assert!(
            db.items_in(LocationId(2), Path::new(ROOT))
                .unwrap()
                .is_empty(),
            "another Location's folder"
        );
    }

    fn enrichment(item: &Item) -> Enrichment {
        Enrichment {
            item: item.id.expect("stored"),
            file: item.identity.file,
            content: item.content.clone(),
            is_placeholder: item.is_placeholder,
            mime: Some("image/png".into()),
            category: TypeCategory::Images,
            is_screenshot: false,
            origin_url: None,
            media: MediaMeta::default(),
        }
    }

    /// The stored Item at `rel`.
    fn stored(db: &IndexDb, rel: &str) -> Item {
        let path = PathBuf::from(ROOT).join(rel);
        db.items_in(LocationId(1), path.parent().unwrap())
            .unwrap()
            .into_iter()
            .find(|i| i.identity.path == path)
            .expect("stored")
    }

    #[test]
    fn items_to_enrich_are_files_and_bundles_not_yet_enriched_paged_by_id() {
        let mut placeholder = item("cloud.pdf", 3);
        placeholder.is_placeholder = true;
        let mut bundle = item("Kipple.app", 0);
        bundle.kind = ItemKind::Bundle;
        let mut dir = item("Sub", 0);
        dir.kind = ItemKind::Dir;
        let mut link = item("latest.pdf", 0);
        link.kind = ItemKind::Symlink;
        let mut metadata = item(".DS_Store", 6);
        metadata.is_os_metadata = true;
        let mut db = db_with(&[
            item("a.png", 1),
            item("b.png", 2),
            placeholder,
            bundle,
            dir,
            link,
            metadata,
            item("done.png", 7),
        ]);
        let mut usb = location();
        usb.id = LocationId(2);
        usb.path = "/Volumes/USB".into();
        db.put_location(&usb).unwrap();
        let mut elsewhere = item("x.png", 8);
        elsewhere.location = LocationId(2);
        elsewhere.identity.path = "/Volumes/USB/x.png".into();
        db.upsert_items(&[elsewhere], Timestamp(1)).unwrap();
        let done = stored(&db, "done.png");
        db.apply_enrichment(&[enrichment(&done)], Timestamp(5))
            .unwrap();

        let mut seen = Vec::new();
        let mut after = None;
        loop {
            let batch = db.items_to_enrich(LocationId(1), after, 2).unwrap();
            assert!(batch.len() <= 2);
            let Some(last) = batch.last() else { break };
            after = last.id;
            seen.extend(batch);
        }
        assert!(seen.windows(2).all(|w| w[0].id < w[1].id), "by id");
        assert_eq!(
            paths(&seen),
            [
                format!("{ROOT}/Kipple.app"),
                format!("{ROOT}/a.png"),
                format!("{ROOT}/b.png"),
                format!("{ROOT}/cloud.pdf"),
            ],
            "not folders, links, OS metadata, enriched items or other Locations"
        );
        assert!(seen.iter().any(|i| i.is_placeholder), "placeholders too");
    }

    #[test]
    fn items_to_enrich_seek_through_their_own_index() {
        let db = db_with(&[]);
        let plan = db
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {}", to_enrich_sql()))
            .unwrap()
            .query_map(params![1, 0, 10], |r| r.get::<_, String>(3))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(plan.contains("item_to_enrich"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "no sort: {plan}");
    }

    /// (mime, category, screenshot, origin, meta_json, enriched_at) as stored.
    type Enriched = (
        Option<String>,
        String,
        bool,
        Option<String>,
        Option<String>,
        Option<i64>,
    );

    fn enriched(db: &IndexDb, id: ItemId) -> Enriched {
        db.conn
            .query_row(
                "SELECT mime, type_category, is_screenshot, origin_url, meta_json, enriched_at \
                 FROM item WHERE id = ?1",
                [id.0],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap()
    }

    #[test]
    fn an_enrichment_stores_what_was_learned() {
        let mut db = db_with(&[item("IMG_0042", 1), item("notes.txt", 2)]);
        let photo = stored(&db, "IMG_0042");
        let notes = stored(&db, "notes.txt");
        let shot = Enrichment {
            mime: Some("image/jpeg".into()),
            category: TypeCategory::Screenshots,
            is_screenshot: true,
            origin_url: Some("https://example.org/a.jpg".into()),
            media: MediaMeta {
                width: Some(4032),
                height: Some(3024),
                taken_at: Some(Timestamp(1_700_000_000_000_000_000)),
                camera_make: Some("Apple".into()),
                camera_model: Some("iPhone \"15\"\n".into()),
                orientation: Some(6),
            },
            ..enrichment(&photo)
        };
        let plain = Enrichment {
            mime: None,
            category: TypeCategory::Documents,
            ..enrichment(&notes)
        };
        assert_eq!(
            db.apply_enrichment(&[shot, plain], Timestamp(9)).unwrap(),
            2
        );
        assert_eq!(
            enriched(&db, photo.id.unwrap()),
            (
                Some("image/jpeg".into()),
                "screenshots".into(),
                true,
                Some("https://example.org/a.jpg".into()),
                Some(
                    r#"{"width":4032,"height":3024,"taken_at":1700000000000000000,"camera_make":"Apple","camera_model":"iPhone \"15\"\n","orientation":6}"#
                        .into()
                ),
                Some(9),
            )
        );
        assert_eq!(
            enriched(&db, notes.id.unwrap()),
            (None, "documents".into(), false, None, None, Some(9)),
            "no media metadata: no JSON"
        );
        assert!(
            db.items_to_enrich(LocationId(1), None, 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_enrichment_raced_by_a_rescan_is_dropped() {
        let mut db = db_with(&[item("a.png", 1), item("b.png", 2), item("c.png", 3)]);
        let read: Vec<_> = ["a.png", "b.png", "c.png"]
            .map(|rel| enrichment(&stored(&db, rel)))
            .into();
        // While the enricher read them: a.png was rewritten, b.png deleted.
        db.upsert_items(&[item("a.png", 100)], Timestamp(2))
            .unwrap();
        db.delete_items(&[read[1].item]).unwrap();
        assert_eq!(db.apply_enrichment(&read, Timestamp(9)).unwrap(), 1);
        assert_eq!(enriched(&db, read[0].item).5, None, "stale: enrich again");
        assert_eq!(enriched(&db, read[2].item).5, Some(9));

        // The same goes for a placeholder that was downloaded meanwhile: its content wasn't read.
        let mut cloud = item("cloud.png", 4);
        cloud.is_placeholder = true;
        db.upsert_items(std::slice::from_ref(&cloud), Timestamp(3))
            .unwrap();
        let seen = enrichment(&stored(&db, "cloud.png"));
        db.upsert_items(&[item("cloud.png", 4)], Timestamp(4))
            .unwrap();
        assert_eq!(db.apply_enrichment(&[seen], Timestamp(9)).unwrap(), 0);
    }

    #[test]
    fn enriching_again_without_finding_an_origin_keeps_the_known_one() {
        let mut db = db_with(&[item("setup.zip", 1)]);
        let first = Enrichment {
            origin_url: Some("https://example.org/setup.zip".into()),
            ..enrichment(&stored(&db, "setup.zip"))
        };
        let id = first.item;
        db.apply_enrichment(&[first], Timestamp(9)).unwrap();
        // Rewritten in place by a save that dropped the attribute: the same file, new content.
        let mut rewritten = item("setup.zip", 2);
        rewritten.identity.file = item("setup.zip", 1).identity.file;
        db.upsert_items(&[rewritten], Timestamp(10)).unwrap();
        assert_eq!(enriched(&db, id).5, None, "enriched again");
        let again = enrichment(&stored(&db, "setup.zip"));
        assert_eq!(db.apply_enrichment(&[again], Timestamp(11)).unwrap(), 1);
        assert_eq!(
            enriched(&db, id).3.as_deref(),
            Some("https://example.org/setup.zip")
        );
    }

    #[test]
    fn another_file_saved_at_the_same_path_does_not_inherit_the_origin() {
        let mut db = db_with(&[item("setup.zip", 1)]);
        let first = Enrichment {
            origin_url: Some("https://example.org/setup.zip".into()),
            ..enrichment(&stored(&db, "setup.zip"))
        };
        let id = first.item;
        db.apply_enrichment(&[first], Timestamp(9)).unwrap();
        // A new file ID: replaced, by a copy, a move or an atomic save.
        db.upsert_items(&[item("setup.zip", 2)], Timestamp(10))
            .unwrap();
        assert_eq!(stored(&db, "setup.zip").id, Some(id), "the same row");
        assert_eq!(enriched(&db, id).3, None);
    }

    #[test]
    fn a_rescan_keeps_the_screenshot_flag_and_the_sniffed_category() {
        let mut db = db_with(&[item("IMG_0042", 1)]);
        let photo = stored(&db, "IMG_0042");
        let shot = Enrichment {
            category: TypeCategory::Screenshots,
            is_screenshot: true,
            ..enrichment(&photo)
        };
        db.apply_enrichment(&[shot], Timestamp(9)).unwrap();
        // The walker knows neither: an unchanged file whose hidden flag changed.
        let mut hidden = item("IMG_0042", 1);
        hidden.is_hidden = true;
        db.upsert_items(&[hidden], Timestamp(10)).unwrap();
        let after = stored(&db, "IMG_0042");
        assert!(after.is_hidden && after.is_screenshot);
        assert_eq!(after.category, TypeCategory::Screenshots);
    }

    #[test]
    fn a_changed_item_takes_the_walkers_category_but_stays_a_screenshot() {
        let mut db = db_with(&[item("Shot.png", 1), item("IMG_0042", 2)]);
        let shot = Enrichment {
            category: TypeCategory::Screenshots,
            is_screenshot: true,
            ..enrichment(&stored(&db, "Shot.png"))
        };
        let photo = Enrichment {
            category: TypeCategory::Screenshots,
            is_screenshot: true,
            ..enrichment(&stored(&db, "IMG_0042"))
        };
        db.apply_enrichment(&[shot, photo], Timestamp(9)).unwrap();
        db.upsert_items(&[item("Shot.png", 11), item("IMG_0042", 12)], Timestamp(10))
            .unwrap();
        let shot = stored(&db, "Shot.png");
        assert!(shot.is_screenshot, "the flag isn't content");
        assert_eq!(shot.category, TypeCategory::Screenshots, "still an image");
        let photo = stored(&db, "IMG_0042");
        assert!(photo.is_screenshot);
        assert_eq!(
            photo.category,
            TypeCategory::Other,
            "the sniffed type is content: the extension's until it's sniffed again"
        );
    }

    #[test]
    fn an_item_not_yet_enriched_takes_the_walkers_category() {
        let mut db = db_with(&[item("a.png", 1)]);
        let mut renamed_type = item("a.png", 1);
        renamed_type.category = TypeCategory::Other;
        renamed_type.is_hidden = true;
        db.upsert_items(&[renamed_type], Timestamp(2)).unwrap();
        assert_eq!(stored(&db, "a.png").category, TypeCategory::Other);
    }

    #[test]
    fn a_downloaded_placeholder_is_enriched_again() {
        let mut cloud = item("cloud.png", 4);
        cloud.is_placeholder = true;
        let mut db = db_with(&[cloud]);
        let seen = enrichment(&stored(&db, "cloud.png"));
        db.apply_enrichment(&[seen], Timestamp(9)).unwrap();
        // Same stamp, now local: its content can be read at last.
        db.upsert_items(&[item("cloud.png", 4)], Timestamp(10))
            .unwrap();
        assert_eq!(
            paths(&db.items_to_enrich(LocationId(1), None, 10).unwrap()),
            [format!("{ROOT}/cloud.png")]
        );
    }

    #[test]
    fn deleting_items_and_everything_under_a_folder() {
        let mut db = db_with(&[
            item("a.pdf", 1),
            item("Sub", 0),
            item("Sub/b.pdf", 2),
            item("Sub/Deeper/c.pdf", 3),
            item("Sub2/d.pdf", 4),
            item("Sub 3/e.pdf", 5),
        ]);
        let a = db.items_in(LocationId(1), Path::new(ROOT)).unwrap();
        let a = a
            .iter()
            .find(|i| i.identity.path.ends_with("a.pdf"))
            .unwrap();
        db.delete_items(&[a.id.unwrap()]).unwrap();
        let sub = Path::new(ROOT).join("Sub");
        db.delete_under(LocationId(1), &sub).unwrap();

        let left: Vec<_> = ["a.pdf", "b.pdf", "c.pdf", "d.pdf", "e.pdf"]
            .into_iter()
            .filter(|name| !db.search(name, None, 10).unwrap().items.is_empty())
            .collect();
        assert_eq!(
            left,
            ["d.pdf", "e.pdf"],
            "only what was inside Sub, and a.pdf"
        );
        assert_eq!(
            paths(&db.items_in(LocationId(1), Path::new(ROOT)).unwrap()),
            [format!("{ROOT}/Sub")],
            "the folder's own row stays: it's deleted like any item"
        );
    }

    #[test]
    fn paths_are_normalised_before_they_reach_the_index() {
        let mut slash = item("a.pdf", 1);
        slash.identity.path = PathBuf::from(format!("{ROOT}/./Sub/a.pdf/"));
        let mut db = db_with(&[slash]);
        let plain = item("Sub/a.pdf", 1);
        db.upsert_items(std::slice::from_ref(&plain), Timestamp(2))
            .unwrap();
        let found = db
            .items_in(LocationId(1), Path::new(&format!("{ROOT}/Sub/")))
            .unwrap();
        assert_eq!(paths(&found), [format!("{ROOT}/Sub/a.pdf")], "one row");
        assert_eq!(names(&db.search("a.pdf", None, 10).unwrap()), ["a.pdf"]);
    }
}
