use crate::model::ScheduleRule;
use chrono::{DateTime, Datelike, Local, Timelike};

pub fn is_schedule_active(rule: &ScheduleRule, now: DateTime<Local>) -> bool {
    if !rule.enabled {
        return true;
    }
    let weekday_idx = now.weekday().num_days_from_monday() as u8; // 0 = Mon .. 6 = Sun
    let minute_now = (now.hour() * 60 + now.minute()) as u16;

    if rule.start_minute <= rule.end_minute {
        let day_ok = (rule.days_mask & (1 << weekday_idx)) != 0;
        day_ok && minute_now >= rule.start_minute && minute_now < rule.end_minute
    } else {
        // Window crosses midnight (e.g. 23:00 -> 06:00)
        if minute_now >= rule.start_minute {
            (rule.days_mask & (1 << weekday_idx)) != 0
        } else if minute_now < rule.end_minute {
            let prev_day = (weekday_idx + 6) % 7;
            (rule.days_mask & (1 << prev_day)) != 0
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn normal_and_overnight_schedule_windows() {
        let rule = ScheduleRule {
            enabled: true,
            days_mask: 0b0000_0001, // Monday only
            start_minute: 23 * 60,
            end_minute: 6 * 60,
        };
        let mon_night = Local.with_ymd_and_hms(2026, 11, 9, 23, 30, 0).unwrap(); // Monday
        let tue_early = Local.with_ymd_and_hms(2026, 11, 10, 2, 15, 0).unwrap(); // Tuesday early AM
        let tue_noon = Local.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap();
        assert!(is_schedule_active(&rule, mon_night));
        assert!(is_schedule_active(&rule, tue_early));
        assert!(!is_schedule_active(&rule, tue_noon));
    }
}
