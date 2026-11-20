//! Local aggregate usage, independent of text history. The controller owns the
//! counters; one ordered writer persists snapshots off the controller task.

use super::{DataRoot, write_atomic};
use crate::ipc::UsageStats;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};
use tokio::{sync::watch, task::JoinHandle};

const STATS_FILE: &str = "stats.json";

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    // A serial Gregorian local day is the key; no timestamp or per-take row.
    daily_words: BTreeMap<i64, u64>,
    words_total: u64,
    takes_total: u64,
    listening_ms: u64,
}

impl Totals {
    fn add(&mut self, day: i64, words: u64, listening_ms: u64) {
        let daily = self.daily_words.entry(day).or_default();
        *daily = daily.saturating_add(words);
        self.words_total = self.words_total.saturating_add(words);
        self.takes_total = self.takes_total.saturating_add(1);
        self.listening_ms = self.listening_ms.saturating_add(listening_ms);
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

    pub(crate) fn record(&mut self, words: u64, listening_ms: u64) {
        self.totals.add(local_day(), words, listening_ms);
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

#[cfg(any(windows, test))]
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
            totals.add(today - age, words, minutes * 60_000);
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
        stats.record(5, 5_000);
        stats.close().await;
        let mut stats = Stats::load(&root);
        stats.record(15, 15_000);
        stats.record(10, 10_000);
        stats.close().await;
        let path = root.dir.join(STATS_FILE);
        let saved = fs::read(&path).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(json["wordsTotal"], 30);
        assert_eq!(json["listeningMs"], 30_000);
        assert_eq!(json.as_object().unwrap().len(), 4);
        let view = Stats::load(&root).view();
        assert_eq!(view.words_this_week, 30);
        assert_eq!(view.takes_total, 3);
        assert_eq!(view.average_wpm, Some(60));

        root.ephemeral = true;
        let mut ephemeral = Stats::load(&root);
        assert_eq!(ephemeral.view(), UsageStats::default());
        ephemeral.record(1, 1_000);
        ephemeral.close().await;
        assert_eq!(ephemeral.view().words_total, 1);
        assert_eq!(fs::read(&path).unwrap(), saved);
        fs::remove_dir_all(&root.dir).unwrap();
    }
}
