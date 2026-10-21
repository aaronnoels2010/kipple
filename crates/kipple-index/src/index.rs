// SPDX-License-Identifier: GPL-3.0-or-later
//! `index.db`: the rebuildable cache of Locations and Items (ADR-0003). WAL with
//! `synchronous=NORMAL`: losing the last moments of a scan only means rescanning.

use std::path::Path;

use kipple_core::category::{TypeCategory, extension_of};
use kipple_core::identity::{ContentHash, ContentStamp, FileId, Identity, VolumeId};
use kipple_core::item::{Item, ItemId, ItemKind};
use kipple_core::location::{Location, LocationId};
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
     full_hash, name";

/// The columns written by a scan, in the order of the staging table and the upsert.
const SCAN_COLUMNS: &str = "location_id, path, parent_path, name, ext, kind, size, created_at, \
     modified_at, changed_at, file_id, volume_id, type_category, is_hidden, is_placeholder, \
     is_screenshot, quick_hash, full_hash, scanned_at";

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

impl IndexDb {
    /// Opens (or creates) the index at `path`. A different schema version means the index is
    /// dropped and recreated empty; the next scan fills it again.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // 64 MiB page cache: the item indexes and the trigram index outgrow the 2 MiB default
        // quickly, and every cache miss is a disk read during a scan.
        conn.pragma_update(None, "cache_size", -65_536)?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        Self::init(conn)
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

    /// Inserts or updates Items by (location, path) in one transaction, returning their IDs in
    /// order. An update keeps the row ID and everything enrichers added (mime, last used,
    /// origin). Stored hashes are kept only while the file ID, size, mtime and ctime are unchanged.
    pub fn upsert_items(&mut self, items: &[Item], scanned_at: Timestamp) -> Result<Vec<ItemId>> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM item_stage", [])?;
        {
            let mut stage = tx.prepare_cached(&format!(
                "INSERT INTO item_stage ({SCAN_COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
                   ?18, ?19)"
            ))?;
            for item in items {
                let path = &item.identity.path;
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
                    scanned_at.0,
                ])?;
            }
        }
        // `WHERE true` keeps SQLite from reading `ON CONFLICT` as part of the SELECT.
        let unchanged = "item.file_id IS excluded.file_id AND item.size = excluded.size \
            AND item.modified_at = excluded.modified_at AND item.changed_at IS excluded.changed_at";
        let mut ids_by_key = std::collections::HashMap::with_capacity(items.len());
        {
            let mut upsert = tx.prepare_cached(&format!(
                "INSERT INTO item ({SCAN_COLUMNS}) SELECT {SCAN_COLUMNS} FROM item_stage WHERE true \
                 ON CONFLICT (location_id, path) DO UPDATE SET parent_path = excluded.parent_path, \
                   name = excluded.name, ext = excluded.ext, kind = excluded.kind, \
                   size = excluded.size, created_at = excluded.created_at, \
                   modified_at = excluded.modified_at, changed_at = excluded.changed_at, \
                   file_id = excluded.file_id, volume_id = excluded.volume_id, \
                   type_category = excluded.type_category, is_hidden = excluded.is_hidden, \
                   is_placeholder = excluded.is_placeholder, is_screenshot = excluded.is_screenshot, \
                   quick_hash = coalesce(excluded.quick_hash, iif({unchanged}, item.quick_hash, NULL)), \
                   full_hash = coalesce(excluded.full_hash, iif({unchanged}, item.full_hash, NULL)), \
                   scanned_at = excluded.scanned_at \
                 RETURNING location_id, path, id"
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
        Ok(items
            .iter()
            .map(|item| ids_by_key[&(item.location.0, path_bytes::encode(&item.identity.path))])
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
        let parent = path_bytes::encode(parent);
        self.page(
            &children_sql(after.is_some()),
            &[&location.0, &parent],
            after,
            limit,
        )
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
                Ok((read_item(r)?, r.get::<_, String>(16)?))
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
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kipple_core::category::TypeCategory;
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
    fn items_round_trip_with_every_field() {
        let mut original = item("Setup.dmg", 128_000_000);
        original.kind = ItemKind::Bundle;
        original.created = Some(Timestamp(-5)); // before 1970 is fine
        original.content.hash = Some(ContentHash::Full([7; 32]));
        original.is_hidden = true;
        original.is_placeholder = true;
        original.is_screenshot = true;
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
        let (mime, last_used): (String, i64) = db
            .conn
            .query_row(
                "SELECT mime, last_used_at FROM item WHERE id = ?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((mime.as_str(), last_used), ("application/pdf", 42));
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
        // Rescanned with a new size: the old hash no longer describes the file.
        db.upsert_items(&[item("a.iso", 11)], Timestamp(3)).unwrap();
        assert_eq!(db.item(id).unwrap().unwrap().content.hash, None);
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
}
