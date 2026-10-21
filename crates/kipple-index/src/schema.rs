// SPDX-License-Identifier: GPL-3.0-or-later
//! The index.db schema (ARCHITECTURE §6.1). The index is a disposable cache, so there are no
//! migrations: when [`SCHEMA_VERSION`] changes, every table is dropped and the next scan
//! rebuilds it.

/// Bump on any schema change. Stored in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i32 = 3;

/// Paths are BLOBs ([`crate::path_bytes`]); times are nanoseconds since the epoch; kinds and
/// categories are the stable codes from `kipple-core`.
pub const DDL: &str = r#"
CREATE TABLE location (
  id                  INTEGER PRIMARY KEY,
  path                BLOB    NOT NULL UNIQUE,
  kind                TEXT    NOT NULL,
  enabled             INTEGER NOT NULL,
  volume_id           TEXT    NOT NULL,
  online              INTEGER NOT NULL,
  last_seen_online_at INTEGER,
  cap_trash           TEXT    NOT NULL,
  cap_case_sensitive  INTEGER NOT NULL,
  cap_stable_ids      INTEGER NOT NULL,
  cap_storage         TEXT    NOT NULL,
  in_sync_root        INTEGER NOT NULL,
  added_at            INTEGER  -- set by location management (K-2.4)
);

CREATE TABLE item (
  id               INTEGER PRIMARY KEY,
  location_id      INTEGER NOT NULL REFERENCES location(id) ON DELETE CASCADE,
  path             BLOB    NOT NULL,
  parent_path      BLOB    NOT NULL,
  name             TEXT    NOT NULL COLLATE NOCASE,  -- so sorting and keyset seeks use one order
  ext              TEXT,
  kind             TEXT    NOT NULL,
  size             INTEGER NOT NULL,
  created_at       INTEGER,
  modified_at      INTEGER NOT NULL,
  changed_at       INTEGER,
  file_id          BLOB,
  volume_id        TEXT    NOT NULL,
  mime             TEXT,
  type_category    TEXT    NOT NULL,
  is_hidden        INTEGER NOT NULL,
  is_placeholder   INTEGER NOT NULL,
  is_screenshot    INTEGER NOT NULL,
  is_os_metadata   INTEGER NOT NULL,  -- `._*`, `.DS_Store`…: left out of counts
  last_used_at     INTEGER,
  last_used_source TEXT,
  origin_url       TEXT,
  quick_hash       BLOB,
  full_hash        BLOB,
  hashed_at        INTEGER,
  phash            BLOB,
  meta_json        TEXT,
  scanned_at       INTEGER NOT NULL,
  enriched_at      INTEGER,
  UNIQUE (location_id, path)
);

-- Folder listings and search results, sorted by name, with keyset pagination.
CREATE INDEX item_children ON item (location_id, parent_path, name, id);
CREATE INDEX item_name ON item (name, id);
CREATE INDEX item_category ON item (type_category);
CREATE INDEX item_size ON item (size);
CREATE INDEX item_full_hash ON item (full_hash) WHERE full_hash IS NOT NULL;
CREATE INDEX item_last_used ON item (last_used_at);
CREATE INDEX item_modified ON item (modified_at);
-- What the enrichers still have to look at, by Location. Partial, so it holds only that
-- backlog: after the first enrich pass it's near empty, and costs a scan nothing to keep up.
CREATE INDEX item_to_enrich ON item (location_id, id)
  WHERE enriched_at IS NULL AND kind IN ('file', 'bundle') AND is_os_metadata = 0;

-- Substring search on names: trigrams, case- and accent-insensitive.
CREATE VIRTUAL TABLE item_name_fts USING fts5(
  name, content = 'item', content_rowid = 'id', tokenize = 'trigram remove_diacritics 1'
);
CREATE TRIGGER item_fts_insert AFTER INSERT ON item BEGIN
  INSERT INTO item_name_fts (rowid, name) VALUES (new.id, new.name);
END;
CREATE TRIGGER item_fts_delete AFTER DELETE ON item BEGIN
  INSERT INTO item_name_fts (item_name_fts, rowid, name) VALUES ('delete', old.id, old.name);
END;
CREATE TRIGGER item_fts_rename AFTER UPDATE OF name ON item WHEN old.name IS NOT new.name BEGIN
  INSERT INTO item_name_fts (item_name_fts, rowid, name) VALUES ('delete', old.id, old.name);
  INSERT INTO item_name_fts (rowid, name) VALUES (new.id, new.name);
END;

CREATE TABLE candidate (
  item_id            INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  detector           TEXT    NOT NULL,
  confidence         TEXT    NOT NULL,
  reason_code        TEXT    NOT NULL,
  reason_params_json TEXT,
  reclaim_bytes      INTEGER NOT NULL,
  group_id           INTEGER,
  computed_at        INTEGER NOT NULL,
  PRIMARY KEY (item_id, detector)
);

CREATE TABLE duplicate_group (
  id                     INTEGER PRIMARY KEY,
  size                   INTEGER NOT NULL,
  full_hash              BLOB    NOT NULL,
  suggested_keep_item_id INTEGER REFERENCES item(id) ON DELETE SET NULL
);
-- Without it, every item delete would scan this table for the foreign key.
CREATE INDEX duplicate_group_keep ON duplicate_group (suggested_keep_item_id);
"#;
