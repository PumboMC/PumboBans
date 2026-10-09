//! Calendar dates for messages and imported files (UTC with a fixed offset).

/// `YYYY-MM-DD HH:MM` for a Unix time in milliseconds, shifted by
/// `offset_minutes` from UTC.
pub fn format_date(ms: u64, offset_minutes: i32) -> String {
    let secs = i64::try_from(ms / 1000).unwrap_or(i64::MAX / 2) + i64::from(offset_minutes) * 60;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, (rem % 3600) / 60)
}

/// [`format_date`] followed by the zone: `2026-10-08 12:00 UTC`, `... UTC+2`.
pub fn format_date_zoned(ms: u64, offset_minutes: i32) -> String {
    let zone = match offset_minutes {
        0 => "UTC".to_string(),
        m if m % 60 == 0 => format!("UTC{:+}", m / 60),
        m => format!("UTC{}{}:{:02}", if m < 0 { '-' } else { '+' }, m.abs() / 60, m.abs() % 60),
    };
    format!("{} {zone}", format_date(ms, offset_minutes))
}

/// Days since 1970-01-01 to a proleptic Gregorian date (H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The inverse of [`civil_from_days`].
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parses the date format of vanilla ban lists, `2026-10-08 13:45:10 +0200`,
/// into Unix milliseconds. `forever` and anything unreadable give `None`.
pub fn parse_vanilla_date(s: &str) -> Option<u64> {
    let mut parts = s.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let zone = parts.next().unwrap_or("+0000");
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: u32 = d.next()?.parse().ok()?;
    let da: u32 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let se: i64 = t.next().unwrap_or("0").parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let digits = zone.trim_start_matches(['+', '-']);
    let zh: i64 = digits.get(..2)?.parse().ok()?;
    let zm: i64 = digits.get(2..4).unwrap_or("00").parse().ok()?;
    let secs = days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + se - sign * (zh * 3600 + zm * 60);
    u64::try_from(secs).ok().map(|s| s * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        assert_eq!(format_date(0, 0), "1970-01-01 00:00");
        let ms = (days_from_civil(2026, 10, 8) * 86_400 + 12 * 3600) as u64 * 1000;
        assert_eq!(format_date(ms, 0), "2026-10-08 12:00");
        assert_eq!(format_date(ms, 120), "2026-10-08 14:00");
        assert_eq!(format_date(ms, -720), "2026-10-08 00:00");
        assert_eq!(format_date_zoned(ms, 0), "2026-10-08 12:00 UTC");
        assert_eq!(format_date_zoned(ms, 120), "2026-10-08 14:00 UTC+2");
        assert_eq!(format_date_zoned(ms, -330), "2026-10-08 06:30 UTC-5:30");
        assert_eq!(parse_vanilla_date("2026-10-08 14:00:00 +0200"), Some(ms));
        assert_eq!(parse_vanilla_date("2026-10-08 07:00:00 -0500"), Some(ms));
        assert_eq!(parse_vanilla_date("forever"), None);
        assert_eq!(parse_vanilla_date("2026-13-08 14:00:00 +0200"), None);
        // leap day
        let leap = (days_from_civil(2028, 2, 29) * 86_400) as u64 * 1000;
        assert_eq!(format_date(leap, 0), "2028-02-29 00:00");
    }
}
