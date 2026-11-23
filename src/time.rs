//! Civil-time helpers. Timestamps are Unix seconds in UTC.
//!
//! The engine deliberately avoids a timezone database: exchange data is stored
//! in UTC, and the same string always maps to the same integer on every host.

/// Seconds in one day.
pub const DAY: i64 = 86_400;

/// Parses `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SSZ` (a trailing `Z` is optional
/// for the date-only form). Returns Unix seconds.
pub fn parse_timestamp(input: &str) -> Result<i64, String> {
    let s = input.trim();
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => match s.split_once(' ') {
            Some((d, t)) => (d, Some(t)),
            None => (s, None),
        },
    };

    let mut dparts = date.split('-');
    let y: i64 = dparts
        .next()
        .ok_or_else(|| format!("invalid date {input:?}"))?
        .parse()
        .map_err(|_| format!("invalid year in {input:?}"))?;
    let m: u32 = dparts
        .next()
        .ok_or_else(|| format!("invalid date {input:?}"))?
        .parse()
        .map_err(|_| format!("invalid month in {input:?}"))?;
    let d: u32 = dparts
        .next()
        .ok_or_else(|| format!("invalid date {input:?}"))?
        .parse()
        .map_err(|_| format!("invalid day in {input:?}"))?;
    if dparts.next().is_some() {
        return Err(format!("invalid date {input:?}"));
    }
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(format!("date component out of range in {input:?}"));
    }
    let days = days_from_civil(y, m, d);
    // Reject impossible dates such as 2023-02-30.
    if civil_from_days(days) != (y, m, d) {
        return Err(format!("invalid calendar date {input:?}"));
    }

    let seconds = match time {
        None => 0,
        Some(t) => {
            let t = t.strip_suffix('Z').unwrap_or(t);
            let mut tparts = t.split(':');
            let hh: i64 = tparts
                .next()
                .ok_or_else(|| format!("invalid time in {input:?}"))?
                .parse()
                .map_err(|_| format!("invalid hour in {input:?}"))?;
            let mm: i64 = tparts
                .next()
                .unwrap_or("0")
                .parse()
                .map_err(|_| format!("invalid minute in {input:?}"))?;
            let ss: i64 = tparts
                .next()
                .unwrap_or("0")
                .parse()
                .map_err(|_| format!("invalid second in {input:?}"))?;
            if tparts.next().is_some()
                || !(0..24).contains(&hh)
                || !(0..60).contains(&mm)
                || !(0..61).contains(&ss)
            {
                return Err(format!("time component out of range in {input:?}"));
            }
            hh * 3_600 + mm * 60 + ss
        }
    };
    Ok(days * DAY + seconds)
}

/// Formats Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn format_timestamp(ts: i64) -> String {
    let days = ts.div_euclid(DAY);
    let secs = ts.rem_euclid(DAY);
    let (y, m, d) = civil_from_days(days);
    let hh = secs / 3_600;
    let mm = (secs % 3_600) / 60;
    let ss = secs % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Formats Unix seconds as `YYYY-MM-DD`.
pub fn format_date(ts: i64) -> String {
    let (y, m, d) = civil_from_days(ts.div_euclid(DAY));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `days_from_civil`, shifted so 1970-01-01 is day 0.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_roundtrip() {
        let ts = parse_timestamp("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(ts, 0);
        assert_eq!(format_timestamp(ts), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_date_roundtrip() {
        let ts = parse_timestamp("2024-02-29T21:00:00Z").unwrap();
        assert_eq!(format_timestamp(ts), "2024-02-29T21:00:00Z");
        assert_eq!(format_date(ts), "2024-02-29");
    }

    #[test]
    fn rejects_impossible_dates() {
        assert!(parse_timestamp("2023-02-29").is_err());
        assert!(parse_timestamp("2024-13-01").is_err());
        assert!(parse_timestamp("2024-01-01T24:00:00Z").is_err());
    }

    #[test]
    fn date_only_is_midnight() {
        assert_eq!(
            parse_timestamp("2024-01-02").unwrap(),
            parse_timestamp("2024-01-02T00:00:00Z").unwrap()
        );
    }
}
