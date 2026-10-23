// SPDX-License-Identifier: GPL-3.0-or-later
//! What the enrichers learned about one Item (ARCHITECTURE §7.1 step 5): its sniffed type,
//! the screenshot flag, the download origin and media metadata.

use crate::category::TypeCategory;
use crate::identity::{ContentStamp, FileId};
use crate::item::ItemId;
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrichment {
    pub item: ItemId,
    /// The file ID, content stamp and placeholder flag as the enricher saw them before reading.
    /// If the index has since recorded other values, a rescan saw a change and what was learned
    /// here is stale, so it isn't stored.
    pub file: Option<FileId>,
    pub content: ContentStamp,
    pub is_placeholder: bool,
    /// As sniffed; `None` when the content wasn't read or wasn't recognised.
    pub mime: Option<String>,
    /// From [`TypeCategory::classify`], so it agrees with `mime` and `is_screenshot`.
    pub category: TypeCategory,
    pub is_screenshot: bool,
    /// Where the file was downloaded from: `kMDItemWhereFroms` on macOS, `Zone.Identifier` on
    /// Windows, `user.xdg.origin.url` on Linux.
    pub origin_url: Option<String>,
    pub media: MediaMeta,
}

/// Dimensions and EXIF of an image. Empty for everything else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaMeta {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub taken_at: Option<Timestamp>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    /// The EXIF orientation (1–8): how the stored pixels are turned.
    pub orientation: Option<u16>,
}
