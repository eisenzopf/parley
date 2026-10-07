use crate::config::Config;
use chrono::{Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WeeklyHours {
    /// 0 = Monday … 6 = Sunday. Empty vec = open all day.
    pub open: Vec<DayWindow>,
    pub timezone: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DayWindow {
    pub weekday: u32,
    pub start_minute: u32,
    pub end_minute: u32,
}

impl Default for WeeklyHours {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            timezone: "UTC".into(),
        }
    }
}

pub fn is_open(config: &Config) -> bool {
    is_open_config(&config.hours, Utc::now())
}

pub fn is_open_config(hours: &crate::config::WeeklyHours, now: chrono::DateTime<Utc>) -> bool {
    if hours.windows.is_empty() {
        return true;
    }
    let weekday = now.weekday().num_days_from_monday() as u8;
    let minute = now.time().hour() * 60 + now.time().minute();
    hours.windows.iter().any(|w| {
        if !w.days.is_empty() && !w.days.contains(&weekday) {
            return false;
        }
        let start = parse_hhmm(&w.start).unwrap_or(0);
        let end = parse_hhmm(&w.end).unwrap_or(24 * 60);
        minute >= start && minute < end
    })
}

fn parse_hhmm(value: &str) -> Option<u32> {
    let (h, m) = value.split_once(':')?;
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

pub fn is_open_at(hours: &WeeklyHours, now: chrono::DateTime<Utc>) -> bool {
    if hours.open.is_empty() {
        return true;
    }
    let weekday = now.weekday().num_days_from_monday();
    let minute = now.time().hour() * 60 + now.time().minute();
    hours
        .open
        .iter()
        .any(|w| w.weekday == weekday && minute >= w.start_minute && minute < w.end_minute)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn empty_windows_are_always_open() {
        let now = Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap();
        assert!(is_open_at(&WeeklyHours::default(), now));
        assert!(is_open_config(&crate::config::WeeklyHours::default(), now));
    }

    #[test]
    fn populated_windows_close_outside_range() {
        let hours = WeeklyHours {
            open: vec![DayWindow {
                weekday: 1,
                start_minute: 9 * 60,
                end_minute: 17 * 60,
            }],
            timezone: "UTC".into(),
        };
        let tuesday_noon = Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap();
        let tuesday_night = Utc.with_ymd_and_hms(2026, 9, 15, 20, 0, 0).unwrap();
        assert!(is_open_at(&hours, tuesday_noon));
        assert!(!is_open_at(&hours, tuesday_night));
        let cfg = crate::config::WeeklyHours {
            timezone: "UTC".into(),
            windows: vec![crate::config::HoursWindow {
                days: vec![],
                start: "00:00".into(),
                end: "00:00".into(),
            }],
        };
        assert!(!is_open_config(&cfg, tuesday_noon));
    }
}
