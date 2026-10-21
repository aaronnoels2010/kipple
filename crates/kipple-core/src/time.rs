// SPDX-License-Identifier: GPL-3.0-or-later
//! Points in time as plain values. The core never reads the clock: callers pass `now` in.

/// Nanoseconds since the Unix epoch (UTC). Nanoseconds, because content stamps compare exact
/// file times; `i64` covers the years 1677 to 2262.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// A time from the OS, clamped like [`Timestamp::from_secs`]. Converting isn't reading the
    /// clock: callers still pass `now` in.
    pub fn from_system_time(time: std::time::SystemTime) -> Self {
        let nanos = |d: std::time::Duration| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX);
        match time.duration_since(std::time::UNIX_EPOCH) {
            Ok(after) => Self(nanos(after)),
            Err(before) => Self(nanos(before.duration()).saturating_neg()),
        }
    }

    /// Seconds since the epoch, clamped to the representable range (1677–2262): junk dates and
    /// Windows' "FILETIME 0" (1601) don't overflow, they pin to the earliest time.
    pub const fn from_secs(secs: i64) -> Self {
        Self(secs.saturating_mul(1_000_000_000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_times_convert_both_sides_of_1970() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(Timestamp::from_system_time(UNIX_EPOCH), Timestamp(0));
        assert_eq!(
            Timestamp::from_system_time(UNIX_EPOCH + Duration::from_nanos(1_500)),
            Timestamp(1_500)
        );
        assert_eq!(
            Timestamp::from_system_time(UNIX_EPOCH - Duration::from_secs(2)),
            Timestamp(-2_000_000_000)
        );
    }

    #[test]
    fn from_secs_saturates_instead_of_overflowing() {
        assert_eq!(Timestamp::from_secs(0), Timestamp(0));
        assert_eq!(
            Timestamp::from_secs(1_700_000_000),
            Timestamp(1_700_000_000_000_000_000)
        );
        assert_eq!(Timestamp::from_secs(-11_644_473_600), Timestamp(i64::MIN)); // FILETIME 0
        assert_eq!(Timestamp::from_secs(i64::MAX), Timestamp(i64::MAX));
    }
}
