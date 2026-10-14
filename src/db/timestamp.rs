//! The one timestamp shape the database stores: UTC, space-separated, with
//! milliseconds and a trailing `Z` (`2026-09-10 12:00:00.000Z`). It is fixed
//! width, so stored values sort and compare correctly as TEXT.

use chrono::{DateTime, NaiveDateTime, Utc};

/// SQL expression for the current time in the stored shape; used as column
/// default. `%f` is seconds with milliseconds.
pub(crate) const SQL_NOW: &str = "strftime('%Y-%m-%d %H:%M:%fZ', 'now')";

pub fn format_db_timestamp(instant: &DateTime<Utc>) -> String {
    instant.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string()
}

/// The current time in the stored shape.
pub fn now_db_timestamp() -> String {
    format_db_timestamp(&Utc::now())
}

/// Reads a stored timestamp. SQLite's own `datetime()` output (no fraction,
/// no `Z`) is UTC as well and reads the same way. `None` for anything else.
pub fn parse_db_timestamp(value: &str) -> Option<DateTime<Utc>> {
    let naive = value.strip_suffix('Z').unwrap_or(value);
    NaiveDateTime::parse_from_str(naive, "%Y-%m-%d %H:%M:%S%.f")
        .ok()
        .map(|t| t.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn at(iso: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn round_trips_the_stored_shape() {
        let instant = at("2026-09-10T12:00:00.250Z");
        let stored = format_db_timestamp(&instant);
        assert_eq!(stored, "2026-09-10 12:00:00.250Z");
        assert_eq!(parse_db_timestamp(&stored), Some(instant));
    }

    #[test]
    fn reads_sqlite_datetime_output_as_utc() {
        assert_eq!(
            parse_db_timestamp("2026-09-10 12:00:00"),
            Some(at("2026-09-10T12:00:00Z"))
        );
    }

    #[test]
    fn rejects_other_values() {
        for value in [
            "",
            "not-a-time",
            "2026-09-10T12:00:00Z",
            "2026-09-32 00:00:00",
        ] {
            assert_eq!(parse_db_timestamp(value), None, "{value}");
        }
    }

    #[test]
    fn the_sql_default_matches_the_rust_shape() {
        let conn = Connection::open_in_memory().unwrap();
        let stored: String = conn
            .query_row(&format!("SELECT {SQL_NOW}"), [], |r| r.get(0))
            .unwrap();
        let parsed = parse_db_timestamp(&stored).unwrap();
        assert_eq!(format_db_timestamp(&parsed), stored);
    }
}
