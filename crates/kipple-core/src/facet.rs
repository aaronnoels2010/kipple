// SPDX-License-Identifier: GPL-3.0-or-later
//! Facets: the other groupings an Item can have several of, next to its one type category
//! (FUNCTIONAL_PLAN §2, CONTEXT.md).

/// Where an Item stands in its life (FUNCTIONAL_PLAN §2, by status).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleStatus {
    /// Used recently.
    Active,
    /// Not used for longer than the configured threshold.
    Stale,
    /// Matched by at least one cleanup detector.
    CleanupCandidate,
    /// The user said never to suggest it again.
    Pinned,
}

/// How an Item got here (v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// From the web, as the OS recorded it (macOS quarantine attribute, Windows Zone.Identifier).
    Downloaded,
    Screenshot,
    CameraImport,
    CreatedLocally,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TagId(pub i64);

/// A stable cleanup detector code, e.g. `installer_old` (ARCHITECTURE §7.3). Shown through the
/// translation catalog, never as text from the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DetectorCode(pub &'static str);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facet {
    Status(LifecycleStatus),
    Origin(Origin),
    Tag(TagId),
    /// A cleanup reason: the detector that flagged the Item.
    Reason(DetectorCode),
}
