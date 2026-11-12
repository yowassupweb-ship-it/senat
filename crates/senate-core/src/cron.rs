//! Five-field cron (`minute hour day-of-month month day-of-week`), evaluated in a fixed UTC offset.
//!
//! Supported per field: `*`, a number, a range `a-b`, a list `a,b,c`, steps `*/n` and `a-b/n`.
//! Day of week is 0–6 with Sunday as 0 (7 is accepted as Sunday too). As in classic cron, when both
//! day-of-month and day-of-week are restricted, a day matching *either* fires.
//! The offset is the caller's business (the node passes the machine's local offset): this module knows
//! nothing about time zones, which keeps it deterministic and testable.

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: Vec<bool>, // 60
    hours: Vec<bool>,   // 24
    dom: Vec<bool>,     // 1..=31 (index 0 unused)
    months: Vec<bool>,  // 1..=12 (index 0 unused)
    dow: Vec<bool>,     // 0..=6
    dom_any: bool,
    dow_any: bool,
}

fn field(src: &str, min: u32, max: u32, what: &str) -> Result<(Vec<bool>, bool)> {
    let bad = |m: &str| Error::invalid(format!("расписание: {what} — {m} («{src}»)"));
    let mut set = vec![false; (max + 1) as usize];
    let mut any = false;
    for part in src.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().map_err(|_| bad("шаг должен быть числом"))?),
            None => (part, 1),
        };
        if step == 0 {
            return Err(bad("шаг не может быть нулём"));
        }
        let (lo, hi) = if range == "*" {
            if step == 1 {
                any = true;
            }
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (a.parse::<u32>().map_err(|_| bad("не число"))?, b.parse::<u32>().map_err(|_| bad("не число"))?)
        } else {
            let n = range.parse::<u32>().map_err(|_| bad("не число"))?;
            (n, if part.contains('/') { max } else { n })
        };
        if lo < min || hi > max || lo > hi {
            return Err(bad(&format!("допустимо от {min} до {max}")));
        }
        let mut v = lo;
        while v <= hi {
            set[v as usize] = true;
            v += step;
        }
    }
    Ok((set, any))
}

impl Cron {
    pub fn parse(expr: &str) -> Result<Cron> {
        let f: Vec<&str> = expr.split_whitespace().collect();
        if f.len() != 5 {
            return Err(Error::invalid("расписание: нужно пять полей — минуты часы число месяц день_недели (например, 0 9 * * 1)"));
        }
        let (minutes, _) = field(f[0], 0, 59, "минуты")?;
        let (hours, _) = field(f[1], 0, 23, "часы")?;
        let (dom, dom_any) = field(f[2], 1, 31, "число месяца")?;
        let (months, _) = field(f[3], 1, 12, "месяц")?;
        // 7 is Sunday too: read the field up to 7, then fold it onto 0
        let (mut dow7, dow_any) = field(f[4], 0, 7, "день недели")?;
        if dow7[7] {
            dow7[0] = true;
        }
        dow7.truncate(7);
        Ok(Cron { minutes, hours, dom, months, dow: dow7, dom_any, dow_any })
    }

    fn day_matches(&self, month: u32, day: u32, weekday: u32) -> bool {
        if !self.months[month as usize] {
            return false;
        }
        let dom_ok = self.dom[day as usize];
        let dow_ok = self.dow[weekday as usize];
        match (self.dom_any, self.dow_any) {
            (true, true) => true,
            (false, true) => dom_ok,
            (true, false) => dow_ok,
            (false, false) => dom_ok || dow_ok,
        }
    }

    /// Every matching minute in [from_ms, to_ms), at most `cap` of them: the plan for a calendar.
    pub fn occurrences(&self, from_ms: i64, to_ms: i64, offset_secs: i64, cap: usize) -> Vec<i64> {
        let mut out = vec![];
        let mut t = from_ms - 1;
        while out.len() < cap {
            match self.next_after(t, offset_secs) {
                Some(n) if n < to_ms => {
                    out.push(n);
                    t = n;
                }
                _ => break,
            }
        }
        out
    }

    /// The first matching minute strictly after `after_ms` (milliseconds since the epoch), in the given offset.
    pub fn next_after(&self, after_ms: i64, offset_secs: i64) -> Option<i64> {
        let start_min = (after_ms.div_euclid(1000) + offset_secs).div_euclid(60) + 1; // local minutes since epoch
        let start_day = start_min.div_euclid(1440);
        for d in 0..(366 * 5) {
            let day = start_day + d;
            let (_, month, dom) = civil_from_days(day);
            let weekday = (day + 4).rem_euclid(7) as u32; // 1970-01-01 was a Thursday
            if !self.day_matches(month, dom, weekday) {
                continue;
            }
            let from_min = if d == 0 { start_min.rem_euclid(1440) } else { 0 };
            for m in from_min..1440 {
                let (h, mi) = ((m / 60) as usize, (m % 60) as usize);
                if self.hours[h] && self.minutes[mi] {
                    let local_min = day * 1440 + m;
                    return Some((local_min * 60 - offset_secs) * 1000);
                }
            }
        }
        None
    }
}

/// "2026-11-16" for a moment, as the wall clock shows it in the offset.
pub fn local_date(ms: i64, offset_secs: i64) -> String {
    let (y, m, d) = civil_from_days((ms.div_euclid(1000) + offset_secs).div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 → (year, month 1–12, day 1–31). Howard Hinnant's civil-from-days.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC milliseconds of a civil date-time (for readable tests).
    fn at(y: i64, m: u32, d: u32, h: i64, mi: i64) -> i64 {
        let y2 = if m <= 2 { y - 1 } else { y };
        let era = y2.div_euclid(400);
        let yoe = y2.rem_euclid(400);
        let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        (days * 86_400 + h * 3600 + mi * 60) * 1000
    }

    #[test]
    fn the_next_monday_at_nine() {
        let c = Cron::parse("0 9 * * 1").unwrap();
        // 2026-11-11 is a Wednesday: the next Monday is the 16th
        assert_eq!(c.next_after(at(2026, 11, 11, 14, 0), 0), Some(at(2026, 11, 16, 9, 0)));
        // exactly at the time: strictly after, so the following week
        assert_eq!(c.next_after(at(2026, 11, 16, 9, 0), 0), Some(at(2026, 11, 23, 9, 0)));
        // one minute before: today
        assert_eq!(c.next_after(at(2026, 11, 16, 8, 59), 0), Some(at(2026, 11, 16, 9, 0)));
    }

    #[test]
    fn the_offset_moves_the_wall_clock_not_the_instant_of_the_rule() {
        // 09:00 in Moscow (UTC+3) is 06:00 UTC
        let c = Cron::parse("0 9 * * *").unwrap();
        assert_eq!(c.next_after(at(2026, 11, 11, 0, 0), 3 * 3600), Some(at(2026, 11, 11, 6, 0)));
        // late in the evening there, already tomorrow in UTC terms
        assert_eq!(c.next_after(at(2026, 11, 11, 21, 30), 3 * 3600), Some(at(2026, 11, 12, 6, 0)));
    }

    #[test]
    fn days_of_month_lists_steps_and_ranges() {
        let c = Cron::parse("30 8 10,20,25 * *").unwrap();
        assert_eq!(c.next_after(at(2026, 11, 11, 0, 0), 0), Some(at(2026, 11, 20, 8, 30)));
        assert_eq!(c.next_after(at(2026, 11, 25, 9, 0), 0), Some(at(2026, 12, 10, 8, 30)));
        let every15 = Cron::parse("*/15 * * * *").unwrap();
        assert_eq!(every15.next_after(at(2026, 11, 11, 14, 1), 0), Some(at(2026, 11, 11, 14, 15)));
        let weekdays = Cron::parse("0 9 * * 1-5").unwrap();
        // Friday evening → Monday
        assert_eq!(weekdays.next_after(at(2026, 11, 13, 18, 0), 0), Some(at(2026, 11, 16, 9, 0)));
        // Thursday and Monday, both
        let two = Cron::parse("0 9 * * 1,4").unwrap();
        assert_eq!(two.next_after(at(2026, 11, 11, 0, 0), 0), Some(at(2026, 11, 12, 9, 0)));
    }

    #[test]
    fn day_and_weekday_together_fire_on_either() {
        // classic cron: the 1st, or any Monday
        let c = Cron::parse("0 6 1 * 1").unwrap();
        assert_eq!(c.next_after(at(2026, 11, 11, 0, 0), 0), Some(at(2026, 11, 16, 6, 0)));
        assert_eq!(c.next_after(at(2026, 11, 27, 0, 0), 0), Some(at(2026, 11, 30, 6, 0)), "Monday the 30th comes before December 1st");
        // Sunday as 0 and as 7
        assert_eq!(Cron::parse("0 6 * * 0").unwrap(), Cron::parse("0 6 * * 7").unwrap());
    }

    #[test]
    fn leap_days_and_year_ends() {
        let c = Cron::parse("0 0 29 2 *").unwrap();
        assert_eq!(c.next_after(at(2026, 11, 11, 0, 0), 0), Some(at(2028, 2, 29, 0, 0)));
        let ny = Cron::parse("0 0 1 1 *").unwrap();
        assert_eq!(ny.next_after(at(2026, 12, 31, 23, 59), 0), Some(at(2027, 1, 1, 0, 0)));
    }

    #[test]
    fn mistakes_are_explained() {
        for bad in ["", "* * * *", "61 * * * *", "* 24 * * *", "* * 0 * *", "* * * 13 *", "* * * * 8", "*/0 * * * *", "a * * * *", "5-1 * * * *"] {
            assert!(Cron::parse(bad).is_err(), "{bad}");
        }
        assert!(Cron::parse("* * * *").unwrap_err().to_string().contains("пять полей"));
    }
}
