//! Epoch seconds to a calendar date, because two crates that never meet write the same partition.
//!
//! Feature rows and cluster membership are both stored partitioned by `dt=YYYY-MM-DD`, and the
//! two crates that write them do not depend on each other. Two copies of date arithmetic drift in
//! silence — the day they disagree is a day one partition holds rows the other does not.
//!
//! No date library is pulled in for it: every timestamp in this contour is a raw epoch second,
//! and a dependency taken for one function is a dependency taken for its whole tree.

/// Epoch seconds in UTC as `YYYY-MM-DD`.
///
/// The arithmetic is Howard Hinnant's `civil_from_days`, which is public domain and states its
/// own proof of range: <http://howardhinnant.github.io/date_algorithms.html>. It is transcribed
/// rather than cited because a reader who has to fetch the page to check a partition name is a
/// reader who will not check it.
#[must_use]
pub fn epoch_seconds_to_ymd(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_is_1970_01_01() {
        assert_eq!(epoch_seconds_to_ymd(0), "1970-01-01");
    }

    #[test]
    fn known_dates_round_trip() {
        assert_eq!(epoch_seconds_to_ymd(1_751_328_000), "2025-07-01");
        assert_eq!(epoch_seconds_to_ymd(946_684_800), "2000-01-01");
        // A leap day, which is where a hand-written civil calendar goes wrong first.
        assert_eq!(epoch_seconds_to_ymd(951_782_400), "2000-02-29");
    }
}
