//! Date formatting — converts `SystemTime` to a `YYYY-MM-DD` string.
//!
//! Uses Howard Hinnant's `civil_from_days` algorithm: exact (leap-year-
//! correct) pure integer math, no `chrono` dependency.
//!
//! References:
//!   * https://howardhinnant.github.io/date_algorithms.html
//!
//! The algorithm converts a count of days since the Unix epoch (1970-01-01)
//! into a (year, month, day) triple. Days are taken as i64 to handle
//! pre-epoch dates gracefully (we just bail out on those via the existing
//! `duration_since(UNIX_EPOCH)` error path).

use std::time::{SystemTime, UNIX_EPOCH};

/// Format a `SystemTime` as `YYYY-MM-DD`. Returns `"?"` for `None`,
/// pre-epoch times, or any other unresolvable input.
pub fn format_date(t: Option<SystemTime>) -> String {
    let t = match t {
        Some(t) => t,
        None => return "?".into(),
    };
    let dur = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "?".into(),
    };
    let days = (dur.as_secs() / 86400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days` — converts days-since-1970-01-01
/// to a proleptic Gregorian (year, month, day) triple.
///
/// Algorithm validity: any `i64` days count. Months are 1-12, days are
/// 1-31. Leap years are handled correctly.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468; // shift epoch from 1970-01-01 to 0000-03-01
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146_097]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        // 0 days since epoch = 1970-01-01
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn epoch_plus_one_day() {
        // 1 day after epoch = 1970-01-02
        assert_eq!(civil_from_days(1), (1970, 1, 2));
    }

    #[test]
    fn epoch_minus_one_day() {
        // 1 day before epoch = 1969-12-31
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn one_year_after_epoch() {
        // 1970 was not a leap year, so 365 days after 1970-01-01 = 1971-01-01
        assert_eq!(civil_from_days(365), (1971, 1, 1));
    }

    #[test]
    fn four_years_after_epoch() {
        // 1972 was a leap year. Days from 1970-01-01 to 1974-01-01:
        //   1970: 365, 1971: 365, 1972: 366, 1973: 365 = 1461
        assert_eq!(civil_from_days(1461), (1974, 1, 1));
    }

    #[test]
    fn leap_year_1972() {
        // 1972 is a leap year (divisible by 4, not by 100).
        // 1972-01-01 = 730 days after 1970-01-01 (365 + 365).
        assert_eq!(civil_from_days(730), (1972, 1, 1));
        // 1972-02-29 = 730 + 31 (Jan) + 28 (Feb 1-28) = 789
        assert_eq!(civil_from_days(789), (1972, 2, 29));
        // 1972-03-01 = 790
        assert_eq!(civil_from_days(790), (1972, 3, 1));
    }

    #[test]
    fn non_leap_year_1973() {
        // 1973 is not a leap year. 1973-02-28 = 1972-12-31 + 31 + 27 = 1125 + 58 = 1125
        // Let's just verify 1973-02-28 exists and 1973-03-01 follows.
        // 1973-01-01 = 1096 (1970 + 1971 + 1972 = 365+365+366 = 1096)
        assert_eq!(civil_from_days(1096), (1973, 1, 1));
        // 1973-02-28 = 1096 + 31 (Jan) + 27 (Feb 1-28 is 28 days, so Feb 28 is +27 from Feb 1)
        // Actually: Jan has 31 days, so Feb 1 = 1096 + 31 = 1127. Feb 28 = 1127 + 27 = 1154.
        assert_eq!(civil_from_days(1154), (1973, 2, 28));
        // 1973-03-01 = 1155 (no Feb 29 in 1973)
        assert_eq!(civil_from_days(1155), (1973, 3, 1));
    }

    #[test]
    fn year_2000_is_leap() {
        // 2000 IS a leap year (divisible by 400, despite being divisible by 100).
        // 2000-01-01: 30 years from 1970. Leap years in 1970-1999: 1972, 76, 80, 84, 88, 92, 96 = 7.
        // Days = 30*365 + 7 = 10_957.
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        // 2000-02-29 = 10_957 + 31 (Jan) + 28 (Feb 1-28) = 11_016
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // 2000-03-01 = 11_017
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }

    #[test]
    fn year_2100_is_not_leap() {
        // 2100 is NOT a leap year (divisible by 100, not by 400).
        // 2100-01-01: 130 years from 1970. Leap years 1972..2096: every 4 years = 32,
        // minus 2100 itself (not in range) = 32. But 2000 was a leap year (div by 400).
        // So count: 1972, 76, ..., 2096 = (2096-1972)/4 + 1 = 32. None excluded in range.
        // Days = 130*365 + 32 = 47_482.
        assert_eq!(civil_from_days(47_482), (2100, 1, 1));
        // 2100-02-28 = 47_482 + 31 + 27 = 47_540
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        // 2100-03-01 = 47_541 (NO Feb 29 in 2100)
        assert_eq!(civil_from_days(47_541), (2100, 3, 1));
    }

    #[test]
    fn year_1900_is_not_leap() {
        // 1900 is NOT a leap year (divisible by 100, not by 400).
        // 1900-01-01 = -25_569 days from 1970-01-01.
        // (1900 to 1970 = 70 years. Leap years 1904..1968 = (1968-1904)/4 + 1 = 17.
        // 1900 is not leap, so 17 leap years in range. Days = 70*365 + 17 = 25_567.
        // So 1900-01-01 = -25_567 from 1970-01-01.)
        assert_eq!(civil_from_days(-25_567), (1900, 1, 1));
        // 1900-02-28 = -25_567 + 31 + 27 = -25_509
        assert_eq!(civil_from_days(-25_509), (1900, 2, 28));
        // 1900-03-01 = -25_508 (no Feb 29 in 1900)
        assert_eq!(civil_from_days(-25_508), (1900, 3, 1));
    }

    #[test]
    fn y2k_rollover() {
        // 1999-12-31 → 2000-01-01 transition.
        // 1999-12-31 = 10_956 (one day before 2000-01-01 = 10_957).
        assert_eq!(civil_from_days(10_956), (1999, 12, 31));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
    }

    #[test]
    fn format_date_none() {
        assert_eq!(format_date(None), "?");
    }

    #[test]
    fn format_date_pre_epoch_returns_question_mark() {
        // Pre-epoch SystemTime can't be expressed as duration_since(UNIX_EPOCH),
        // so format_date returns "?" for them.
        let t = Some(SystemTime::UNIX_EPOCH - std::time::Duration::from_secs(86400));
        assert_eq!(format_date(t), "?");
    }

    #[test]
    fn format_date_produces_yyyy_mm_dd_shape() {
        let t = Some(SystemTime::now());
        let s = format_date(t);
        assert_eq!(s.len(), 10, "expected YYYY-MM-DD (10 chars), got: {s}");
        assert_eq!(s.chars().nth(4), Some('-'));
        assert_eq!(s.chars().nth(7), Some('-'));
        // Year should start with 2 (we're in the 2000s for the foreseeable future)
        assert_eq!(s.chars().next(), Some('2'));
    }
}
