// SPDX-License-Identifier: GPL-3.0-or-later
//! Items: what the index records about each file, folder, symlink or bundle (CONTEXT.md).

use crate::category::TypeCategory;
use crate::identity::{ContentStamp, Identity};
use crate::location::LocationId;
use crate::time::Timestamp;

/// The index's row ID for an Item. Only meaningful inside one index (it's rebuilt by a rescan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    File,
    Dir,
    Symlink,
    /// A folder the OS shows as one file (`.app`, `.photoslibrary`): never descended into.
    Bundle,
}

/// What the index records about an Item. Its path is `identity.path`; the name, extension and
/// parent folder are derived from it where the index needs them as columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `None` until the index has stored it.
    pub id: Option<ItemId>,
    pub location: LocationId,
    pub kind: ItemKind,
    pub identity: Identity,
    pub content: ContentStamp,
    pub created: Option<Timestamp>,
    pub category: TypeCategory,
    pub is_hidden: bool,
    /// Online-only cloud file: its contents are never read (that would download it).
    pub is_placeholder: bool,
    pub is_screenshot: bool,
}
