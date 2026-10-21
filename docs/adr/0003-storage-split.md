# ADR-0003: Split storage into disposable index, precious state and portable config

- Status: Accepted (revised after architecture review)
- Date: 2026-10-20

## Context
Kipple stores three kinds of data with very different lifecycles:
- **File metadata** (millions of rows) that can always be rebuilt by rescanning.
- **The journal, plans, marks and tags**: user decisions and history that can't be rebuilt. Losing them breaks undo, recovery, "never suggest again" and tagging.
- **Settings, rules and presets**, which users want to export, import and sync between computers without a server.

## Decision
- **`index.db`** (SQLite, WAL, `synchronous=NORMAL`) is a disposable cache, including the FTS5 name index. On an incompatible schema change it may be deleted and rebuilt.
- **`state.db`** (SQLite, WAL, **`synchronous=FULL`**) holds plans, the journal, marks, **tags**, stats and location size history. It gets forward-only migrations, with an automatic backup copy before each migration.
- **`config/*.json`** holds human-readable, schema-versioned settings, rules, rename presets and tidy presets. Export/import works on these files, and the config dir can be pointed at a user-synced folder. Imported rules always arrive disabled.
- Thumbnails and logs live in the cache and log dirs and are disposable.
- **Amendment (K-2.4): Locations live in `<app data dir>/locations.json`**, written atomically. They are user decisions that a rescan can't rebuild, so they can't live only in the disposable `index.db`, whose `location` table now just mirrors the file (refilled at startup, rows not in the file removed). They aren't in the portable `config/` folder either: paths and volume IDs belong to one computer, and syncing them to another would point at the wrong folders.
- **"Remove all Kipple data"** in settings deletes all of these. This is needed because the `.dmg`, AppImage and portable builds have no uninstaller.

## Consequences
- The index schema can evolve fast without risky migrations.
- Precious data is small and durable.
- Config sync works through any folder sync, with no Kipple server.
- User data (marks, tags, journal) references items by `(volume_id, file_id)` with a path fallback, never by index row IDs.
