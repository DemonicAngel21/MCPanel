//! Grandfather-father-son retention for scheduled backups.
//!
//! Backups are considered newest first. A backup is kept if it is one of the newest
//! `keep_last`, or the newest backup of one of the `keep_daily` most recent days (or
//! ISO weeks / months) that have backups. Buckets use the user's local time zone.
//! Everything else is expired. All-zero retention keeps everything.

use super::Retention;
use crate::ids::BackupId;
use crate::time::Timestamp;
use jiff::tz::TimeZone;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub id: BackupId,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Buckets {
    day: (i16, i8, i8),
    week: (i16, i8),
    month: (i16, i8),
}

fn buckets(t: Timestamp, tz: &TimeZone) -> Buckets {
    let ts = jiff::Timestamp::from_millisecond(t.millis()).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let date = ts.to_zoned(tz.clone()).date();
    let week = date.iso_week_date();
    Buckets {
        day: (date.year(), date.month(), date.day()),
        week: (week.year(), week.week()),
        month: (date.year(), date.month()),
    }
}

/// Ids of the backups that retention removes.
pub fn expired(candidates: &[Candidate], r: Retention, tz: &TimeZone) -> Vec<BackupId> {
    if r.keep_last == 0 && r.keep_daily == 0 && r.keep_weekly == 0 && r.keep_monthly == 0 {
        return Vec::new();
    }
    let mut sorted: Vec<Candidate> = candidates.to_vec();
    sorted.sort_by_key(|c| std::cmp::Reverse((c.created_at, c.id)));

    let mut keep: HashSet<BackupId> = HashSet::new();
    keep.extend(sorted.iter().take(r.keep_last as usize).map(|c| c.id));

    let with_buckets: Vec<(Candidate, Buckets)> = sorted
        .iter()
        .map(|c| (*c, buckets(c.created_at, tz)))
        .collect();
    let mut keep_by = |limit: u32, key: &dyn Fn(&Buckets) -> (i16, i8, i8)| {
        let mut last = None;
        let mut count = 0;
        for (c, b) in &with_buckets {
            if count >= limit {
                break;
            }
            let k = key(b);
            if last != Some(k) {
                keep.insert(c.id);
                last = Some(k);
                count += 1;
            }
        }
    };
    keep_by(r.keep_daily, &|b| b.day);
    keep_by(r.keep_weekly, &|b| (b.week.0, b.week.1, 0));
    keep_by(r.keep_monthly, &|b| (b.month.0, b.month.1, 0));

    sorted
        .iter()
        .filter(|c| !keep.contains(&c.id))
        .map(|c| c.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;

    fn at(ms: i64) -> Candidate {
        Candidate {
            id: BackupId::new(),
            created_at: Timestamp(ms),
        }
    }

    // 2026-01-05 00:00 UTC is a Monday.
    const MONDAY: i64 = 1_767_571_200_000;

    fn r(last: u32, daily: u32, weekly: u32, monthly: u32) -> Retention {
        Retention {
            keep_last: last,
            keep_daily: daily,
            keep_weekly: weekly,
            keep_monthly: monthly,
        }
    }

    #[test]
    fn keep_last_keeps_the_newest() {
        let c: Vec<_> = (0..5).map(|i| at(MONDAY + i * HOUR)).collect();
        let gone = expired(&c, r(2, 0, 0, 0), &TimeZone::UTC);
        assert_eq!(gone.len(), 3);
        assert!(!gone.contains(&c[4].id) && !gone.contains(&c[3].id));
    }

    #[test]
    fn daily_keeps_the_newest_backup_of_each_day() {
        // Four backups per day for 10 days.
        let c: Vec<_> = (0..10)
            .flat_map(|d| (0..4).map(move |h| at(MONDAY + d * DAY + h * 6 * HOUR)))
            .collect();
        let gone: HashSet<_> = expired(&c, r(0, 7, 0, 0), &TimeZone::UTC)
            .into_iter()
            .collect();
        let kept: Vec<_> = c.iter().filter(|x| !gone.contains(&x.id)).collect();
        assert_eq!(kept.len(), 7);
        // Each kept backup is the last of its day (18:00).
        assert!(
            kept.iter()
                .all(|k| (k.created_at.0 - MONDAY) % DAY == 18 * HOUR)
        );
        // The newest 7 days.
        assert!(kept.iter().all(|k| k.created_at.0 >= MONDAY + 3 * DAY));
    }

    #[test]
    fn weekly_and_monthly_reach_further_back() {
        // One backup per day for 90 days.
        let c: Vec<_> = (0..90).map(|d| at(MONDAY + d * DAY + 12 * HOUR)).collect();
        let gone: HashSet<_> = expired(&c, r(1, 3, 4, 3), &TimeZone::UTC)
            .into_iter()
            .collect();
        let kept: Vec<i64> = c
            .iter()
            .filter(|x| !gone.contains(&x.id))
            .map(|x| (x.created_at.0 - MONDAY) / DAY)
            .collect();
        // Day 0 is Mon 2026-01-05, day 89 is Sat 2026-04-04.
        // days: newest 3 (87, 88, 89); weeks: newest of the last 4 ISO weeks (89, then the
        // Sundays 83, 76, 69); months: newest of the last 3 months (Apr → 89,
        // Mar 31 = 85, Feb 28 = 54).
        let mut want = vec![87, 88, 89, 83, 76, 69, 85, 54];
        want.sort();
        let mut got = kept.clone();
        got.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn zero_retention_keeps_everything() {
        let c: Vec<_> = (0..5).map(|i| at(MONDAY + i * DAY)).collect();
        assert!(expired(&c, r(0, 0, 0, 0), &TimeZone::UTC).is_empty());
    }

    #[test]
    fn local_time_zone_decides_the_day() {
        // 23:30 and 00:30 UTC are the same day in UTC-2 (21:30 / 22:30 the day before).
        let a = at(MONDAY - 30 * 60_000);
        let b = at(MONDAY + 30 * 60_000);
        let tz = TimeZone::fixed(jiff::tz::offset(-2));
        assert_eq!(expired(&[a, b], r(0, 5, 0, 0), &tz), vec![a.id]);
        assert!(expired(&[a, b], r(0, 5, 0, 0), &TimeZone::UTC).is_empty());
    }
}
