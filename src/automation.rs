//! Headless capture automation: interval / wall-clock scheduling, region
//! change detection, and the configurable after-capture workflow chain.
//!
//! Everything here is pure (no GUI, no network, no threads) so the decision
//! logic can be unit-tested on any host. The app layer (`poll_automation`) owns
//! the clock and performs the real I/O the plan calls for.

use image::RgbaImage;
use std::time::Duration;

/// How often an unattended capture fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    /// Automation is off.
    Off,
    /// Fire every `secs` seconds.
    Interval { secs: u64 },
    /// Fire once a day at each local wall-clock `(hour, minute)`.
    Daily { times: Vec<(u8, u8)> },
}

/// Validated `(hour, minute)` pairs, sorted and de-duplicated. Invalid entries
/// (hour ≥ 24, minute ≥ 60) are dropped rather than poisoning the whole plan.
pub fn normalise_times(times: &[(u8, u8)]) -> Vec<(u8, u8)> {
    let mut out: Vec<(u8, u8)> = times
        .iter()
        .copied()
        .filter(|&(h, m)| h < 24 && m < 60)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Parse a `HH:MM` token (also tolerates `H:M`). Returns `None` for garbage.
pub fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u8 = h.trim().parse().ok()?;
    let m: u8 = m.trim().parse().ok()?;
    if h < 24 && m < 60 { Some((h, m)) } else { None }
}

impl Schedule {
    /// Seconds until the next automatic capture.
    ///
    /// `now` is local `(hour, minute, second)`; `elapsed` is how long since the
    /// last interval firing. Returns `None` when nothing should ever fire, so
    /// the caller can skip its repaint budget entirely.
    pub fn next_in(&self, now: (u8, u8, u8), elapsed: Duration) -> Option<Duration> {
        match self {
            Schedule::Off => None,
            Schedule::Interval { secs } => {
                if *secs == 0 {
                    return None;
                }
                let period = Duration::from_secs(*secs);
                Some(period.saturating_sub(elapsed).max(Duration::from_secs(1)))
            }
            Schedule::Daily { times } => {
                let valid = normalise_times(times);
                if valid.is_empty() {
                    return None;
                }
                let (h, m, s) = now;
                let now_secs = h as u64 * 3600 + m as u64 * 60 + s as u64;
                let day = 86_400_u64;
                let mut best: Option<u64> = None;
                for &(th, tm) in &valid {
                    let target = th as u64 * 3600 + tm as u64 * 60;
                    // Fire at the start of the minute; if that instant already
                    // passed today, roll to the same time tomorrow.
                    let delta = if target > now_secs {
                        target - now_secs
                    } else {
                        day - now_secs + target
                    };
                    best = Some(best.map_or(delta, |b: u64| b.min(delta)));
                }
                best.map(|d| Duration::from_secs(d.max(1)))
            }
        }
    }
}

/// Whether a scheduled capture is due at this instant.
///
/// - `Interval`: `elapsed` since the last firing reached the period.
/// - `Daily`: the current local `(hour, minute)` matches a configured time and
///   the last firing was more than a minute ago (so a single minute fires once,
///   even though `ui()` ticks many times within it).
/// - `Off` / `Daily` with no times: never.
///
/// `last_fire_ago` is the elapsed time since the last automatic capture.
pub fn is_due(schedule: &Schedule, now: (u8, u8, u8), last_fire_ago: Duration) -> bool {
    match schedule {
        Schedule::Off => false,
        Schedule::Interval { secs } => *secs > 0 && last_fire_ago >= Duration::from_secs(*secs),
        Schedule::Daily { times } => {
            let valid = normalise_times(times);
            let minute = (now.0, now.1);
            let matches = valid.contains(&minute);
            // A 60 s guard prevents double firing inside one matching minute.
            matches && last_fire_ago >= Duration::from_secs(60)
        }
    }
}

/// Mean absolute per-channel difference between two equally-sized RGBA frames,
/// sampled every `step` pixels (and every `step` rows) for speed. Returns
/// `None` when the dimensions differ (a monitor was reconfigured mid-watch).
/// `0.0` = identical, `255.0` = black vs. white.
pub fn frame_diff(a: &RgbaImage, b: &RgbaImage, step: u32) -> Option<f32> {
    if a.dimensions() != b.dimensions() {
        return None;
    }
    let (w, h) = a.dimensions();
    let step = step.max(1);
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let pa = a.get_pixel(x, y).0;
            let pb = b.get_pixel(x, y).0;
            // Ignore alpha: desktop captures are opaque and a transient alpha
            // change must not trigger a capture.
            for c in 0..3 {
                sum += (pa[c] as i32 - pb[c] as i32).unsigned_abs() as u64;
            }
            count += 3;
            x += step;
        }
        y += step;
    }
    if count == 0 {
        Some(0.0)
    } else {
        Some(sum as f32 / count as f32)
    }
}

/// Whether the watched region changed enough to justify a capture. A dimension
/// change counts as a change; `threshold` is on the same 0–255 scale as
/// [`frame_diff`].
pub fn region_changed(prev: &RgbaImage, next: &RgbaImage, threshold: f32, step: u32) -> bool {
    match frame_diff(prev, next, step) {
        None => true,
        // A strictly positive difference is required: with a 0.0 threshold a
        // pair of identical frames (diff == 0.0) must NOT count as a change,
        // otherwise the monitor would fire every tick on a static screen.
        Some(d) => d > 0.0 && d >= threshold.max(0.0),
    }
}

/// One step in an after-capture workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowAction {
    /// Save once in the default configured format.
    Save,
    /// Save an extra copy in a specific format.
    SaveExtra(ExtraFormat),
    /// Put the image on the clipboard.
    CopyImage,
    /// Upload via the configured backend.
    Upload,
    /// Copy the resulting share link to the clipboard (implies an upload).
    CopyLink,
    /// Show a completion toast / play the notification sound.
    Notify,
}

/// Extra output formats for a workflow, kept independent of `util::ImageFormat`
/// so this module stays free of I/O dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraFormat {
    Jpeg,
    Webp,
    Bmp,
    Tiff,
    Gif,
}

impl ExtraFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ExtraFormat::Jpeg => "jpeg",
            ExtraFormat::Webp => "webp",
            ExtraFormat::Bmp => "bmp",
            ExtraFormat::Tiff => "tiff",
            ExtraFormat::Gif => "gif",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => Some(ExtraFormat::Jpeg),
            "webp" => Some(ExtraFormat::Webp),
            "bmp" => Some(ExtraFormat::Bmp),
            "tiff" | "tif" => Some(ExtraFormat::Tiff),
            "gif" => Some(ExtraFormat::Gif),
            _ => None,
        }
    }
}

impl WorkflowAction {
    /// Stable, human-editable token used in `config.json` and the CLI.
    pub fn code(self) -> &'static str {
        use WorkflowAction::*;
        match self {
            Save => "save",
            SaveExtra(f) => match f {
                ExtraFormat::Jpeg => "save:jpeg",
                ExtraFormat::Webp => "save:webp",
                ExtraFormat::Bmp => "save:bmp",
                ExtraFormat::Tiff => "save:tiff",
                ExtraFormat::Gif => "save:gif",
            },
            CopyImage => "copy-image",
            Upload => "upload",
            CopyLink => "copy-link",
            Notify => "notify",
        }
    }

    pub fn from_code(s: &str) -> Option<Self> {
        use WorkflowAction::*;
        match s.trim().to_ascii_lowercase().as_str() {
            "save" | "save:png" => Some(Save),
            "copy-image" => Some(CopyImage),
            "upload" => Some(Upload),
            "copy-link" => Some(CopyLink),
            "notify" => Some(Notify),
            other => other
                .strip_prefix("save:")
                .and_then(ExtraFormat::from_str)
                .map(SaveExtra),
        }
    }

    /// Parse a whole workflow from tokens, ignoring unknown ones.
    pub fn parse_all(tokens: &[String]) -> Vec<Self> {
        tokens.iter().filter_map(|t| Self::from_code(t)).collect()
    }
}

/// Normalise a user-authored workflow into a deterministic execution plan:
/// - `CopyLink` always gets a preceding `Upload` (a link without an upload is
///   meaningless) and is moved after it;
/// - duplicate `Save` / `CopyImage` / `Upload` / `Notify` steps are dropped;
/// - `Save` stays first and `Notify` last, so the chain reads naturally;
/// - the whole plan is capped so a corrupt config cannot spawn unbounded work.
pub fn plan(actions: &[WorkflowAction]) -> Vec<WorkflowAction> {
    use WorkflowAction::*;
    const LIMIT: usize = 16;

    let mut want_save = false;
    let mut extras: Vec<ExtraFormat> = Vec::new();
    let mut want_copy_image = false;
    let mut want_upload = false;
    let mut want_copy_link = false;
    let mut want_notify = false;

    for &a in actions.iter().take(LIMIT * 2) {
        match a {
            Save => want_save = true,
            SaveExtra(f) => {
                if !extras.contains(&f) {
                    extras.push(f);
                }
            }
            CopyImage => want_copy_image = true,
            Upload => want_upload = true,
            CopyLink => {
                want_copy_link = true;
                want_upload = true; // implied dependency
            }
            Notify => want_notify = true,
        }
    }

    let mut out = Vec::with_capacity(LIMIT);
    if want_save {
        out.push(Save);
    }
    for f in extras {
        out.push(SaveExtra(f));
    }
    if want_copy_image {
        out.push(CopyImage);
    }
    if want_upload {
        out.push(Upload);
    }
    if want_copy_link {
        out.push(CopyLink);
    }
    if want_notify {
        out.push(Notify);
    }
    out.truncate(LIMIT);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn parse_hhmm_accepts_and_rejects() {
        assert_eq!(parse_hhmm("09:30"), Some((9, 30)));
        assert_eq!(parse_hhmm("9:5"), Some((9, 5)));
        assert_eq!(parse_hhmm(" 23:59 "), Some((23, 59)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("nope"), None);
        assert_eq!(parse_hhmm("12"), None);
    }

    #[test]
    fn normalise_times_sorts_dedups_and_validates() {
        let got = normalise_times(&[(25, 0), (9, 5), (9, 5), (0, 0), (12, 61)]);
        assert_eq!(got, vec![(0, 0), (9, 5)]);
    }

    #[test]
    fn off_and_empty_schedules_never_fire() {
        assert_eq!(Schedule::Off.next_in((12, 0, 0), Duration::ZERO), None);
        assert_eq!(
            Schedule::Interval { secs: 0 }.next_in((12, 0, 0), Duration::ZERO),
            None
        );
        assert_eq!(
            Schedule::Daily { times: vec![] }.next_in((12, 0, 0), Duration::ZERO),
            None
        );
        assert_eq!(
            Schedule::Daily {
                times: vec![(25, 0)]
            }
            .next_in((12, 0, 0), Duration::ZERO),
            None
        );
    }

    #[test]
    fn interval_countdown_clamps_to_one_second() {
        assert_eq!(
            Schedule::Interval { secs: 30 }.next_in((0, 0, 0), Duration::ZERO),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            Schedule::Interval { secs: 30 }.next_in((0, 0, 0), Duration::from_secs(12)),
            Some(Duration::from_secs(18))
        );
        // Already due → still wait a token second so the loop cannot spin.
        assert_eq!(
            Schedule::Interval { secs: 30 }.next_in((0, 0, 0), Duration::from_secs(99)),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn daily_picks_the_nearest_future_time() {
        // 09:00 and 17:00; at 08:59:50 the next is 09:00 in 10 seconds.
        let s = Schedule::Daily {
            times: vec![(17, 0), (9, 0)],
        };
        assert_eq!(
            s.next_in((8, 59, 50), Duration::ZERO),
            Some(Duration::from_secs(10))
        );
        // At 10:00 the next is 17:00 (7 hours).
        assert_eq!(
            s.next_in((10, 0, 0), Duration::ZERO),
            Some(Duration::from_secs(7 * 3600))
        );
        // After the last time of the day, roll over to tomorrow's first.
        assert_eq!(
            s.next_in((18, 0, 0), Duration::ZERO),
            Some(Duration::from_secs(15 * 3600))
        );
    }

    #[test]
    fn daily_exact_instant_rolls_to_tomorrow() {
        let s = Schedule::Daily {
            times: vec![(9, 0)],
        };
        assert_eq!(
            s.next_in((9, 0, 0), Duration::ZERO),
            Some(Duration::from_secs(86_400))
        );
    }

    #[test]
    fn is_due_for_intervals_and_daily_minutes() {
        let interval = Schedule::Interval { secs: 30 };
        assert!(!is_due(&interval, (0, 0, 0), Duration::from_secs(29)));
        assert!(is_due(&interval, (0, 0, 0), Duration::from_secs(30)));
        assert!(!is_due(
            &Schedule::Interval { secs: 0 },
            (0, 0, 0),
            Duration::from_secs(999)
        ));
        assert!(!is_due(&Schedule::Off, (0, 0, 0), Duration::from_secs(999)));

        let daily = Schedule::Daily {
            times: vec![(9, 30), (17, 0)],
        };
        assert!(is_due(&daily, (9, 30, 12), Duration::from_secs(120)));
        // Same minute but fired 20 s ago → suppress the duplicate.
        assert!(!is_due(&daily, (9, 30, 40), Duration::from_secs(20)));
        assert!(!is_due(&daily, (9, 31, 0), Duration::from_secs(999)));
        assert!(is_due(&daily, (17, 0, 5), Duration::from_secs(3600)));
    }

    #[test]
    fn frame_diff_is_zero_for_identical_and_max_for_black_white() {
        let a = RgbaImage::from_pixel(40, 40, Rgba([10, 20, 30, 255]));
        assert_eq!(frame_diff(&a, &a, 4), Some(0.0));
        let black = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 255]));
        let white = RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255]));
        assert_eq!(frame_diff(&black, &white, 4), Some(255.0));
    }

    #[test]
    fn frame_diff_detects_a_small_changed_patch() {
        let a = RgbaImage::from_pixel(100, 100, Rgba([0, 0, 0, 255]));
        let mut b = a.clone();
        // A 10x10 white patch = 1% of pixels → mean ≈ 2.55 on the red channel.
        for y in 0..10 {
            for x in 0..10 {
                b.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let d = frame_diff(&a, &b, 1).unwrap();
        assert!(d > 0.0 && d < 10.0, "diff was {d}");
        assert!(region_changed(&a, &b, 1.0, 1));
        assert!(!region_changed(&a, &b, 50.0, 1));
        assert!(!region_changed(&a, &a, 0.0, 1));
    }

    #[test]
    fn identical_frames_never_trigger_even_at_zero_threshold() {
        let a = RgbaImage::from_pixel(20, 20, Rgba([7, 8, 9, 255]));
        assert!(!region_changed(&a, &a, 0.0, 1));
        assert!(!region_changed(&a, &a, 0.0, 4));
    }

    #[test]
    fn dimension_change_counts_as_a_change() {
        let a = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        let b = RgbaImage::from_pixel(11, 10, Rgba([0, 0, 0, 255]));
        assert_eq!(frame_diff(&a, &b, 1), None);
        assert!(region_changed(&a, &b, 999.0, 1));
    }

    #[test]
    fn plan_orders_dedups_and_adds_implied_upload() {
        use WorkflowAction::*;
        let got = plan(&[
            Notify,
            CopyLink,
            CopyImage,
            SaveExtra(ExtraFormat::Jpeg),
            SaveExtra(ExtraFormat::Jpeg),
            Save,
            Save,
            Notify,
        ]);
        assert_eq!(
            got,
            vec![
                Save,
                SaveExtra(ExtraFormat::Jpeg),
                CopyImage,
                Upload,
                CopyLink,
                Notify
            ]
        );
    }

    #[test]
    fn plan_empty_and_self_contained() {
        assert!(plan(&[]).is_empty());
        assert_eq!(
            plan(&[WorkflowAction::CopyLink]),
            vec![WorkflowAction::Upload, WorkflowAction::CopyLink]
        );
    }

    #[test]
    fn extra_format_round_trips() {
        for f in [
            ExtraFormat::Jpeg,
            ExtraFormat::Webp,
            ExtraFormat::Bmp,
            ExtraFormat::Tiff,
            ExtraFormat::Gif,
        ] {
            assert_eq!(ExtraFormat::from_str(f.as_str()), Some(f));
        }
        assert_eq!(ExtraFormat::from_str("jpg"), Some(ExtraFormat::Jpeg));
        assert_eq!(ExtraFormat::from_str("nope"), None);
    }

    #[test]
    fn workflow_codes_round_trip() {
        use WorkflowAction::*;
        let all = [
            Save,
            SaveExtra(ExtraFormat::Jpeg),
            SaveExtra(ExtraFormat::Webp),
            SaveExtra(ExtraFormat::Bmp),
            SaveExtra(ExtraFormat::Tiff),
            SaveExtra(ExtraFormat::Gif),
            CopyImage,
            Upload,
            CopyLink,
            Notify,
        ];
        for a in all {
            assert_eq!(
                WorkflowAction::from_code(a.code()),
                Some(a),
                "code {}",
                a.code()
            );
        }
        assert_eq!(WorkflowAction::from_code("save:png"), Some(Save));
        assert_eq!(WorkflowAction::from_code("nope"), None);
    }

    #[test]
    fn parse_all_ignores_unknown_tokens() {
        let tokens: Vec<String> = ["save", "bogus", "copy-link", "notify"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            WorkflowAction::parse_all(&tokens),
            vec![
                WorkflowAction::Save,
                WorkflowAction::CopyLink,
                WorkflowAction::Notify
            ]
        );
    }
}
