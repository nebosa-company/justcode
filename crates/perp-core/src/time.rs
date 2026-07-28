//! UTC timestamps, without a dependency.
//!
//! Formatting takes the epoch second as an argument rather than reading the
//! clock, so every test in the crate is deterministic (NFR §2: no clock
//! dependence). Only the CLI calls [`now`].

/// `YYYY-MM-DDTHH:MM:SSZ` for an epoch second, before or after 1970.
pub fn format_utc(unix_secs: i64) -> String {
    let days = div_floor(unix_secs, 86_400);
    let secs_of_day = unix_secs - days * 86_400;
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) =
        (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// `YYYY-MM-DD` for an epoch second — what the state file and the board date
/// themselves with.
pub fn format_date(unix_secs: i64) -> String {
    let (year, month, day) = civil_from_days(div_floor(unix_secs, 86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Seconds since the epoch. Returns 0 rather than failing if the system clock
/// is before 1970 in a way `SystemTime` cannot express — a wrong timestamp is
/// worth recording; a panic here would take the batch down (`N-9`).
pub fn now() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

fn div_floor(a: i64, b: i64) -> i64 {
    let q = a / b;
    if a % b != 0 && (a < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

/// Days since 1970-01-01 to a civil date. Hinnant's algorithm, which is exact
/// for the proleptic Gregorian calendar and needs no table.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = div_floor(z, 146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_epoch() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_a_known_instant() {
        assert_eq!(format_utc(1_000_000_000), "2001-09-09T01:46:40Z");
    }

    #[test]
    fn handles_the_end_of_a_leap_year() {
        assert_eq!(format_utc(1_609_459_199), "2020-12-31T23:59:59Z");
        assert_eq!(format_utc(1_609_459_200), "2021-01-01T00:00:00Z");
    }

    #[test]
    fn handles_a_leap_day() {
        assert_eq!(format_utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn handles_before_the_epoch() {
        // Naive integer division truncates toward zero and would produce
        // 1970-01-01T-1 here.
        assert_eq!(format_utc(-1), "1969-12-31T23:59:59Z");
        assert_eq!(format_utc(-86_400), "1969-12-31T00:00:00Z");
    }

    #[test]
    fn dates_without_the_time() {
        assert_eq!(format_date(1_774_000_000), "2026-03-20");
    }
}
