//! Frame-time statistics for the scripted scroll budget check (spec section 8: 60 fps).

use std::time::{Duration, Instant};

/// `DUET_AUTOSCROLL_SECS=N`: scroll ~1,000 rows per frame for N seconds, log frame stats, quit.
pub(crate) struct ScrollProbe {
    limit: Duration,
    started: Option<Instant>,
    last: Option<Instant>,
    gaps: Vec<Duration>,
    /// The text view's scroll target; the hex view moves its own top row.
    pub(crate) row: usize,
}

impl ScrollProbe {
    pub(crate) fn from_env() -> Option<Self> {
        let secs: u64 = std::env::var("DUET_AUTOSCROLL_SECS").ok()?.parse().ok()?;
        Some(Self {
            limit: Duration::from_secs(secs),
            started: None,
            last: None,
            gaps: Vec::new(),
            row: 0,
        })
    }

    /// Counts one frame. Once the time is up, logs the frame stats and returns true: the view
    /// quits then.
    pub(crate) fn frame(&mut self) -> bool {
        let now = Instant::now();
        let started = *self.started.get_or_insert(now);
        if let Some(last) = self.last.replace(now) {
            self.gaps.push(now - last);
        }
        if now - started <= self.limit {
            return false;
        }
        let s = frame_stats(&self.gaps);
        tracing::info!(
            frames = s.frames,
            mean_ms = s.mean_ms,
            p95_ms = s.p95_ms,
            max_ms = s.max_ms,
            gaps_over_two_vsyncs = s.gaps_over_two_vsyncs,
            "autoscroll"
        );
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    pub frames: usize,
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    /// Frames that took longer than two 60 Hz vsyncs: the ones a user sees as stutter.
    pub gaps_over_two_vsyncs: usize,
}

/// Gaps of a quarter second or more are idle time (no frame was requested), not frames.
pub fn frame_stats(gaps: &[Duration]) -> FrameStats {
    let mut ms: Vec<f64> = gaps
        .iter()
        .filter(|g| **g < Duration::from_millis(250))
        .map(|g| g.as_secs_f64() * 1e3)
        .collect();
    if ms.is_empty() {
        return FrameStats {
            frames: 0,
            mean_ms: 0.0,
            p95_ms: 0.0,
            max_ms: 0.0,
            gaps_over_two_vsyncs: 0,
        };
    }
    ms.sort_by(f64::total_cmp);
    let frames = ms.len();
    FrameStats {
        frames,
        mean_ms: ms.iter().sum::<f64>() / frames as f64,
        p95_ms: ms[(frames * 95 / 100).min(frames - 1)],
        max_ms: ms[frames - 1],
        gaps_over_two_vsyncs: ms.iter().filter(|m| **m > 33.4).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_sixty_fps_has_no_stutter() {
        let gaps = vec![Duration::from_micros(16_667); 120];
        let stats = frame_stats(&gaps);
        assert_eq!(stats.frames, 120);
        assert!((stats.mean_ms - 16.667).abs() < 0.01);
        assert_eq!(stats.gaps_over_two_vsyncs, 0);
    }

    #[test]
    fn long_frames_count_and_idle_gaps_are_ignored() {
        let mut gaps = vec![Duration::from_millis(16); 98];
        gaps.push(Duration::from_millis(40));
        gaps.push(Duration::from_secs(2));
        let stats = frame_stats(&gaps);
        assert_eq!(stats.frames, 99);
        assert_eq!(stats.gaps_over_two_vsyncs, 1);
        assert!((stats.max_ms - 40.0).abs() < 1e-9);
        assert_eq!(frame_stats(&[]).frames, 0);
    }
}
