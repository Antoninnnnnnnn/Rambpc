//! Local aggregate usage, independent of text history. The controller owns the
//! counters; one ordered writer persists snapshots off the controller task.

use super::{DataRoot, write_atomic};
use crate::{
    ipc::{ActivityDay, ActivityStats, AppWords, UsageStats},
    settings::AppIdentity,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};
use tokio::{sync::watch, task::JoinHandle};

const STATS_FILE: &str = "stats.json";

/// Days of per-day counters kept (the activity screen shows 112).
const KEEP_DAYS: i64 = 800;
/// Days returned by `get_activity`.
const ACTIVITY_DAYS: i64 = 112;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct Totals {
    // A serial Gregorian local day is the key; no timestamp or per-take row.
    daily_words: BTreeMap<i64, u64>,
    /// Completed takes per local day (added in 0.10; older files start empty).
    daily_takes: BTreeMap<i64, u64>,
    // Local serial day -> executable id -> display name and word count.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    daily_apps: BTreeMap<i64, BTreeMap<String, AppCount>>,
    words_total: u64,
    takes_total: u64,
    listening_ms: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct AppCount {
    name: String,
    words: u64,
}

impl Totals {
    fn add(&mut self, day: i64, words: u64, listening_ms: u64, app: Option<AppIdentity>) {
        let daily = self.daily_words.entry(day).or_default();
        *daily = daily.saturating_add(words);
        let takes = self.daily_takes.entry(day).or_default();
        *takes = takes.saturating_add(1);
        if let Some(app) = app {
            let count = self
                .daily_apps
                .entry(day)
                .or_default()
                .entry(app.id)
                .or_default();
            count.name = app.name;
            count.words = count.words.saturating_add(words);
        }
        let oldest = day.saturating_sub(KEEP_DAYS);
        self.daily_words.retain(|d, _| *d > oldest);
        self.daily_takes.retain(|d, _| *d > oldest);
        self.daily_apps.retain(|d, _| *d > oldest);
        self.words_total = self.words_total.saturating_add(words);
        self.takes_total = self.takes_total.saturating_add(1);
        self.listening_ms = self.listening_ms.saturating_add(listening_ms);
    }

    fn active(&self, day: i64) -> bool {
        self.daily_takes.get(&day).is_some_and(|t| *t > 0)
            || self.daily_words.get(&day).is_some_and(|w| *w > 0)
    }

    fn app_words(&self, days: impl std::ops::RangeBounds<i64>) -> Vec<AppWords> {
        let mut totals: BTreeMap<&str, (&str, u64)> = BTreeMap::new();
        for (_, apps) in self.daily_apps.range(days) {
            for (id, count) in apps {
                let (name, words) = totals.entry(id).or_default();
                *name = &count.name; // Newest display name in the requested range.
                *words = words.saturating_add(count.words);
            }
        }
        let mut apps: Vec<_> = totals
            .into_iter()
            .map(|(id, (name, words))| AppWords {
                app: name.to_owned(),
                words,
                id: id.to_owned(),
            })
            .collect();
        apps.sort_unstable_by(|a, b| b.words.cmp(&a.words).then_with(|| a.id.cmp(&b.id)));
        apps
    }

    fn activity(&self, today: i64) -> ActivityStats {
        let window = today - (ACTIVITY_DAYS - 1)..=today;
        let days = window.clone()
            .map(|day| ActivityDay {
                date: date_string(day),
                words: self.daily_words.get(&day).copied().unwrap_or_default(),
                takes: self.daily_takes.get(&day).copied().unwrap_or_default(),
            })
            .collect();
        let mut current_streak = 0_u32;
        let mut cursor = if self.active(today) { today } else { today - 1 };
        while self.active(cursor) {
            current_streak += 1;
            cursor -= 1;
        }
        let mut active_days: Vec<i64> = self
            .daily_words
            .keys()
            .chain(self.daily_takes.keys())
            .copied()
            .filter(|day| self.active(*day))
            .collect();
        active_days.sort_unstable();
        active_days.dedup();
        let (mut longest, mut longest_end, mut run, mut previous) = (0_u32, None, 0_u32, None);
        for day in &active_days {
            run = if previous == Some(day - 1) { run + 1 } else { 1 };
            if run >= longest {
                longest = run;
                longest_end = Some(*day);
            }
            previous = Some(*day);
        }
        ActivityStats {
            days,
            current_streak,
            longest_streak: longest,
            longest_streak_end: longest_end.map(date_string),
            first_day: active_days.first().copied().map(date_string),
            listening_ms_total: self.listening_ms,
            apps: self
                .daily_apps
                .values()
                .any(|apps| !apps.is_empty())
                .then(|| self.app_words(window)),
        }
    }

    fn view(&self, today: i64) -> UsageStats {
        UsageStats {
            words_today: self.daily_words.get(&today).copied().unwrap_or_default(),
            words_this_week: self
                .daily_words
                .range(today.saturating_sub(6)..=today)
                .fold(0_u64, |sum, (_, words)| sum.saturating_add(*words)),
            words_total: self.words_total,
            takes_total: self.takes_total,
            average_wpm: (self.listening_ms != 0).then(|| {
                let ms = u128::from(self.listening_ms);
                let rounded = (u128::from(self.words_total) * 60_000 + ms / 2) / ms;
                u32::try_from(rounded).unwrap_or(u32::MAX)
            }),
        }
    }
}

#[derive(Default)]
pub(crate) struct Stats {
    totals: Totals,
    path: Option<PathBuf>,
    writer: Option<(watch::Sender<Totals>, JoinHandle<()>)>,
}

impl Stats {
    /// Startup reads before the controller is spawned. Ephemeral mode neither
    /// reads nor modifies an existing stats file in the data root.
    pub(crate) fn load(root: &DataRoot) -> Self {
        if root.ephemeral {
            return Self::default();
        }
        let path = root.dir.join(STATS_FILE);
        let totals = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            totals,
            path: Some(path),
            writer: None,
        }
    }

    pub(crate) fn record(&mut self, words: u64, listening_ms: u64, app: Option<AppIdentity>) {
        self.totals.add(local_day(), words, listening_ms, app);
        if let Some(path) = self.path.take() {
            // A watch slot coalesces queued snapshots, never reorders writes,
            // and bounds memory even on slow portable storage.
            let (tx, mut rx) = watch::channel(self.totals.clone());
            rx.mark_changed();
            let worker = tokio::spawn(async move {
                while rx.changed().await.is_ok() {
                    let snapshot = rx.borrow_and_update().clone();
                    let path = path.clone();
                    let saved = tokio::task::spawn_blocking(move || {
                        let bytes = serde_json::to_vec(&snapshot)?;
                        write_atomic(&path, &bytes)
                    })
                    .await;
                    if !matches!(saved, Ok(Ok(()))) {
                        eprintln!("Sambler: usage statistics could not be saved");
                    }
                }
            });
            self.writer = Some((tx, worker));
        } else if let Some((tx, _)) = &self.writer {
            tx.send_replace(self.totals.clone());
        }
    }

    pub(crate) fn view(&self) -> UsageStats {
        self.totals.view(local_day())
    }

    pub(crate) fn activity(&self) -> ActivityStats {
        self.totals.activity(local_day())
    }

    pub(crate) fn known_apps(&self) -> Vec<AppIdentity> {
        self.totals
            .app_words(..)
            .into_iter()
            .map(|app| AppIdentity {
                id: app.id,
                name: app.app,
            })
            .collect()
    }

    pub(crate) async fn close(&mut self) {
        if let Some((tx, worker)) = self.writer.take() {
            drop(tx);
            let _ = worker.await;
        }
    }
}

// Use the OS calendar, not UTC midnight or a fixed 24-hour lookback. The
// Foundation struct is already linked into the Windows host; GetLocalTime
// does not otherwise need the Windows crate's SystemInformation feature.
#[cfg(windows)]
fn local_day() -> i64 {
    use windows::Win32::Foundation::SYSTEMTIME;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut SYSTEMTIME);
    }
    let mut time = SYSTEMTIME::default();
    // SAFETY: the Win32 API fills this writable SYSTEMTIME synchronously.
    unsafe { GetLocalTime(&mut time) };
    civil_day(time.wYear, time.wMonth, time.wDay)
}

#[cfg(not(windows))]
fn local_day() -> i64 {
    // Only the Windows host runs the app; use UTC for non-Windows unit builds.
    (std::time::UNIX_EPOCH
        .elapsed()
        .unwrap_or_default()
        .as_secs()
        / 86_400) as i64
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn civil_day(year: u16, month: u16, day: u16) -> i64 {
    let preceding_years = i64::from(year) - 1;
    let days_before_year =
        preceding_years * 365 + preceding_years / 4 - preceding_years / 100 + preceding_years / 400;
    let days_before_month = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    days_before_year
        + days_before_month[usize::from(month - 1)]
        + i64::from(leap && month > 2)
        + i64::from(day)
}

/// `YYYY-MM-DD` for a serial day produced by `civil_day`.
fn date_string(day: i64) -> String {
    // Howard Hinnant's civil_from_days, on days since 1970-01-01.
    let z = day - civil_day(1970, 1, 1) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Edition;

    #[test]
    fn aggregates_calendar_days_and_weights_average_by_listening_duration() {
        let mut totals = Totals::default();
        let today = civil_day(2024, 3, 3);
        assert_eq!(today - 3, civil_day(2024, 2, 29));
        assert_eq!(civil_day(2025, 1, 1) - 1, civil_day(2024, 12, 31));
        assert_eq!(civil_day(2100, 3, 1) - 1, civil_day(2100, 2, 28));
        for (age, words, minutes) in [(-1, 15, 1), (0, 60, 1), (6, 24, 2), (7, 7, 1)] {
            totals.add(today - age, words, minutes * 60_000, None);
        }
        assert_eq!(
            totals.view(today),
            UsageStats {
                words_today: 60,
                words_this_week: 84,
                words_total: 106,
                takes_total: 4,
                average_wpm: Some(21),
            }
        );
        assert_eq!(Totals::default().view(today).average_wpm, None);
    }

    #[tokio::test]
    async fn persists_numeric_aggregates_and_keeps_ephemeral_counters_in_memory() {
        let dir = std::env::temp_dir().join(format!("sambler-stats-{}", uuid::Uuid::new_v4()));
        let mut root = DataRoot {
            edition: Edition::Portable,
            webview_dir: dir.join("WebView2"),
            dir,
            ephemeral: false,
        };
        let mut stats = Stats::load(&root);
        let word = AppIdentity { id: "winword".into(), name: "Word".into() };
        stats.record(5, 5_000, Some(word.clone()));
        stats.close().await;
        let mut stats = Stats::load(&root);
        stats.record(15, 15_000, None);
        stats.record(10, 10_000, Some(word.clone()));
        stats.close().await;
        let path = root.dir.join(STATS_FILE);
        let saved = fs::read(&path).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(json["wordsTotal"], 30);
        assert_eq!(json["listeningMs"], 30_000);
        assert_eq!(json.as_object().unwrap().len(), 6);
        assert_eq!(json["dailyApps"][local_day().to_string()], serde_json::json!({
            "winword": {"name": "Word", "words": 15}
        }));
        let reloaded = Stats::load(&root);
        assert_eq!(reloaded.known_apps(), [word]);
        let view = reloaded.view();
        assert_eq!(view.words_this_week, 30);
        assert_eq!(view.takes_total, 3);
        assert_eq!(view.average_wpm, Some(60));

        root.ephemeral = true;
        let mut ephemeral = Stats::load(&root);
        assert_eq!(ephemeral.view(), UsageStats::default());
        assert!(ephemeral.known_apps().is_empty());
        ephemeral.record(1, 1_000, None);
        ephemeral.close().await;
        assert_eq!(ephemeral.view().words_total, 1);
        assert_eq!(fs::read(&path).unwrap(), saved);
        fs::remove_dir_all(&root.dir).unwrap();
    }

    #[test]
    fn activity_days_streaks_and_dates() {
        assert_eq!(date_string(civil_day(2024, 2, 29)), "2024-02-29");
        assert_eq!(date_string(civil_day(1999, 12, 31)), "1999-12-31");
        assert_eq!(date_string(civil_day(2026, 11, 21)), "2026-11-21");
        let today = civil_day(2026, 11, 21);
        let mut totals = Totals::default();
        // A 3-day run ending 10 days ago, then today missing but yesterday and
        // the day before active: current streak 2, longest 3.
        for age in [12, 11, 10, 2, 1] {
            totals.add(today - age, 10, 60_000, None);
        }
        let activity = totals.activity(today);
        assert_eq!(activity.days.len(), 112);
        assert_eq!(activity.days.last().unwrap().date, "2026-11-21");
        assert_eq!(activity.days[111 - 1].takes, 1);
        assert_eq!(activity.current_streak, 2);
        assert_eq!(activity.longest_streak, 3);
        assert_eq!(activity.longest_streak_end.as_deref(), Some("2026-11-11"));
        assert_eq!(activity.first_day.as_deref(), Some("2026-11-09"));
        assert_eq!(activity.listening_ms_total, 300_000);
        assert!(activity.apps.is_none());
        totals.add(today, 5, 1_000, None);
        assert_eq!(totals.activity(today).current_streak, 3);
        // Counters older than the retention window are pruned.
        totals.add(today + KEEP_DAYS + 30, 1, 1_000, None);
        assert!(totals.daily_takes.keys().all(|d| *d > today + 30));
    }

    #[test]
    fn app_activity_uses_112_days_but_known_apps_use_all_retained_days() {
        let today = civil_day(2026, 11, 21);
        let mut totals = Totals::default();
        for (age, id, name, words) in [
            (-1, "future", "Future", 10), // The local clock can move back.
            (KEEP_DAYS, "pruned", "Pruned", 1000),
            (KEEP_DAYS - 1, "old", "Old", 50),
            (112, "firefox", "Firefox", 90),
            (111, "firefox", "Old description", 15),
            (1, "notepad", "Notepad", 10),
            (0, "firefox", "Firefox", 5),
        ] {
            totals.add(today - age, words, 1_000, Some(AppIdentity { id: id.into(), name: name.into() }));
        }
        totals.add(today, 100, 1_000, None); // Recents-only also contribute to overall usage.
        assert_eq!(totals.view(today).words_today, 105);
        assert_eq!(totals.activity(today).apps.unwrap(), [
            AppWords { id: "firefox".into(), app: "Firefox".into(), words: 20 },
            AppWords { id: "notepad".into(), app: "Notepad".into(), words: 10 },
        ]);
        assert_eq!(totals.activity(today + ACTIVITY_DAYS + 1).apps, Some(vec![]));
        let stats = Stats { totals, ..Stats::default() };
        let ids: Vec<_> = stats.known_apps().into_iter().map(|app| app.id).collect();
        assert_eq!(ids, ["firefox", "old", "future", "notepad"]);
        // The day 799 days back was retained at `today`, but expires one day later.
        let mut totals = stats.totals;
        totals.add(today + 1, 1, 1_000, None);
        assert!(!totals.daily_apps.contains_key(&(today - KEEP_DAYS + 1)));
    }

    #[test]
    fn older_files_without_app_counters_keep_their_usage() {
        let old: Totals = serde_json::from_str(
            r#"{"dailyWords":{"739000":5},"wordsTotal":5,"takesTotal":1,"listeningMs":1000}"#,
        )
        .unwrap();
        assert!(old.daily_takes.is_empty());
        assert!(old.activity(739000).apps.is_none());
        assert_eq!(old.view(739000).words_today, 5);
        assert_eq!(old.view(739000).takes_total, 1);
    }
}
