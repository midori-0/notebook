//! Timestamps: UTC, RFC 3339, whole seconds. As text they sort in time order, which is why
//! `--since` and `--until` accept any prefix.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`, via Howard Hinnant's `civil_from_days`.
pub fn format(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_utc() {
        assert_eq!(super::format(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::format(951868800), "2000-03-01T00:00:00Z");
        assert_eq!(super::format(1762689600), "2025-11-09T12:00:00Z");
    }
}
