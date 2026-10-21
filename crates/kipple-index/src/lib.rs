// SPDX-License-Identifier: GPL-3.0-or-later
//! SQLite storage: index.db (rebuildable cache) and state.db (journal, marks, tags).

#![forbid(unsafe_code)]

mod index;
mod path_bytes;
mod schema;

pub use index::{Cursor, IndexDb, Page};
pub use schema::SCHEMA_VERSION;
