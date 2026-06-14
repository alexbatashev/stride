//! Minimal 5-field cron parser. Granularity is one minute; the scheduler ticks
//! frequently and asks whether a schedule matches the current wall-clock minute.

#[derive(Clone, Copy, Debug)]
struct Field {
    any: bool,
    mask: u64,
}

impl Field {
    fn matches(&self, value: u32) -> bool {
        self.any || (self.mask >> value) & 1 == 1
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cron {
    minute: Field,
    hour: Field,
    dom: Field,
    month: Field,
    dow: Field,
}

impl Cron {
    pub fn parse(expr: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err("cron expression must have 5 fields".to_string());
        }
        Ok(Cron {
            minute: parse_field(fields[0], 0, 59)?,
            hour: parse_field(fields[1], 0, 23)?,
            dom: parse_field(fields[2], 1, 31)?,
            month: parse_field(fields[3], 1, 12)?,
            dow: parse_field(fields[4], 0, 6)?,
        })
    }

    /// Whether the schedule fires at the given UTC unix timestamp (minute precision).
    pub fn matches_at(&self, unix_secs: i64) -> bool {
        let (minute, hour, dom, month, dow) = decompose(unix_secs);
        if !(self.minute.matches(minute) && self.hour.matches(hour) && self.month.matches(month)) {
            return false;
        }
        let d = self.dom.matches(dom);
        let w = self.dow.matches(dow);
        // POSIX cron: when both day-of-month and day-of-week are restricted, the
        // entry fires if either matches; otherwise both must match.
        if !self.dom.any && !self.dow.any {
            d || w
        } else {
            d && w
        }
    }
}

fn parse_field(spec: &str, min: u32, max: u32) -> Result<Field, String> {
    if spec == "*" {
        return Ok(Field { any: true, mask: 0 });
    }
    let mut mask = 0u64;
    for part in spec.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().map_err(|_| bad(part))?),
            None => (part, 1),
        };
        if step == 0 {
            return Err(bad(part));
        }
        let (start, end) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (parse_num(a, max)?, parse_num(b, max)?)
        } else {
            let v = parse_num(range, max)?;
            // A bare number with a step (e.g. `5/10`) runs from the number to max.
            if part.contains('/') { (v, max) } else { (v, v) }
        };
        if start < min || end > max || start > end {
            return Err(bad(part));
        }
        let mut v = start;
        while v <= end {
            mask |= 1 << v;
            v += step;
        }
    }
    Ok(Field { any: false, mask })
}

fn parse_num(s: &str, max: u32) -> Result<u32, String> {
    let v: u32 = s.parse().map_err(|_| bad(s))?;
    // Day-of-week accepts 7 as an alias for Sunday.
    Ok(if max == 6 && v == 7 { 0 } else { v })
}

fn bad(part: &str) -> String {
    format!("invalid cron field: {part}")
}

/// Returns (minute, hour, day-of-month, month, day-of-week) for a UTC timestamp.
/// Day-of-week is 0=Sunday..6=Saturday.
fn decompose(unix_secs: i64) -> (u32, u32, u32, u32, u32) {
    let secs = unix_secs.max(0);
    let days = (secs / 86400) as u32;
    let secs_of_day = (secs % 86400) as u32;
    let minute = (secs_of_day / 60) % 60;
    let hour = secs_of_day / 3600;
    // 1970-01-01 was a Thursday (=4 when Sunday=0).
    let dow = (days + 4) % 7;

    // Hinnant's civil_from_days algorithm.
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };

    (minute, hour, d, m, dow)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-06-14 12:34:00 UTC (a Sunday).
    const SUNDAY_NOON: i64 = 1_781_440_440;

    #[test]
    fn decompose_known_timestamp() {
        let (minute, hour, dom, month, dow) = decompose(SUNDAY_NOON);
        assert_eq!((minute, hour, dom, month, dow), (34, 12, 14, 6, 0));
    }

    #[test]
    fn wildcard_matches_every_minute() {
        let cron = Cron::parse("* * * * *").unwrap();
        assert!(cron.matches_at(SUNDAY_NOON));
        assert!(cron.matches_at(0));
    }

    #[test]
    fn exact_minute_and_hour() {
        let cron = Cron::parse("34 12 * * *").unwrap();
        assert!(cron.matches_at(SUNDAY_NOON));
        assert!(!cron.matches_at(SUNDAY_NOON + 60));
        assert!(!cron.matches_at(SUNDAY_NOON + 3600));
    }

    #[test]
    fn step_values() {
        let cron = Cron::parse("*/15 * * * *").unwrap();
        assert!(cron.matches_at(SUNDAY_NOON - 4 * 60)); // minute 30
        assert!(!cron.matches_at(SUNDAY_NOON)); // minute 34
    }

    #[test]
    fn list_and_range() {
        let cron = Cron::parse("0,34 9-17 * * *").unwrap();
        assert!(cron.matches_at(SUNDAY_NOON));
        let nine_zero = SUNDAY_NOON - 34 * 60 - 3 * 3600; // 09:00
        assert!(cron.matches_at(nine_zero));
    }

    #[test]
    fn day_of_week_name() {
        let sunday = Cron::parse("34 12 * * 0").unwrap();
        assert!(sunday.matches_at(SUNDAY_NOON));
        let monday = Cron::parse("34 12 * * 1").unwrap();
        assert!(!monday.matches_at(SUNDAY_NOON));
        let sunday_alias = Cron::parse("34 12 * * 7").unwrap();
        assert!(sunday_alias.matches_at(SUNDAY_NOON));
    }

    #[test]
    fn dom_or_dow_when_both_restricted() {
        // Either the 14th of the month or any Monday.
        let cron = Cron::parse("34 12 14 * 1").unwrap();
        assert!(cron.matches_at(SUNDAY_NOON)); // matches day-of-month 14
        let cron2 = Cron::parse("34 12 1 * 0").unwrap();
        assert!(cron2.matches_at(SUNDAY_NOON)); // matches Sunday even though dom is 1
    }

    #[test]
    fn dom_and_dow_when_one_wild() {
        let cron = Cron::parse("34 12 13 * *").unwrap();
        assert!(!cron.matches_at(SUNDAY_NOON)); // dom is 14, not 13
    }

    #[test]
    fn rejects_bad_expressions() {
        assert!(Cron::parse("* * * *").is_err());
        assert!(Cron::parse("60 * * * *").is_err());
        assert!(Cron::parse("* 24 * * *").is_err());
        assert!(Cron::parse("a * * * *").is_err());
        assert!(Cron::parse("*/0 * * * *").is_err());
    }
}
