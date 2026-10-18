//! Time-based window animation. The platform supplies display ticks and applies
//! only the latest sample; missed refreshes never build a queue of old frames.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::{geometry::Rect, WindowId};

#[derive(Clone, Copy)]
struct Tween {
    from: Rect,
    to: Rect,
    started: Instant,
    duration: Duration,
}

impl Tween {
    fn sample(self, now: Instant) -> Rect {
        let t = (now.saturating_duration_since(self.started).as_secs_f64()
            / self.duration.as_secs_f64())
        .clamp(0.0, 1.0);
        // Ease out without overshoot: intermediate sizes remain between two
        // constraint-checked endpoints.
        let eased = 1.0 - (1.0 - t).powi(3);
        let mix = |a: f64, b: f64| (a + (b - a) * eased).round();
        if t >= 1.0 {
            self.to
        } else {
            Rect::new(
                mix(self.from.x, self.to.x),
                mix(self.from.y, self.to.y),
                mix(self.from.w, self.to.w),
                mix(self.from.h, self.to.h),
            )
        }
    }
}

#[derive(Default)]
pub struct Motion {
    tweens: BTreeMap<WindowId, Tween>,
}

impl Motion {
    /// Retarget from the last frame actually submitted to AX, not a theoretical
    /// frame that may never have been displayed while the event loop was busy.
    pub fn retarget(
        &mut self,
        id: WindowId,
        from: Rect,
        to: Rect,
        now: Instant,
        duration: Duration,
    ) {
        if from.approx_eq(&to) || duration.is_zero() {
            self.tweens.remove(&id);
        } else {
            self.tweens.insert(
                id,
                Tween {
                    from,
                    to,
                    started: now,
                    duration,
                },
            );
        }
    }

    pub fn samples(&mut self, now: Instant) -> Vec<(WindowId, Rect, bool)> {
        let frames = self
            .tweens
            .iter()
            .map(|(&id, tween)| {
                let done = now.saturating_duration_since(tween.started) >= tween.duration;
                (id, tween.sample(now), done)
            })
            .collect();
        self.tweens
            .retain(|_, tween| now.saturating_duration_since(tween.started) < tween.duration);
        frames
    }

    pub fn contains(&self, id: WindowId) -> bool {
        self.tweens.contains_key(&id)
    }
    pub fn is_empty(&self) -> bool {
        self.tweens.is_empty()
    }
    pub fn remove(&mut self, id: WindowId) {
        self.tweens.remove(&id);
    }
    pub fn finish(&mut self) -> Vec<(WindowId, Rect)> {
        std::mem::take(&mut self.tweens)
            .into_iter()
            .map(|(id, tween)| (id, tween.to))
            .collect()
    }
    pub fn clear(&mut self) {
        self.tweens.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: Rect = Rect::new(0.0, 30.0, 500.0, 600.0);
    const TO: Rect = Rect::new(900.0, 40.0, 800.0, 900.0);

    #[test]
    fn a_missed_refresh_skips_to_current_time_and_finishes_exactly() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.retarget(1, FROM, TO, now, Duration::from_millis(160));
        let (_, mid, done) = motion.samples(now + Duration::from_millis(80))[0];
        assert!(!done);
        assert!(mid.x > 450.0 && mid.x < TO.x);
        assert!(mid.w >= FROM.w && mid.w <= TO.w);
        assert_eq!(
            motion.samples(now + Duration::from_secs(1)),
            vec![(1, TO, true)]
        );
        assert!(motion.is_empty());
    }

    #[test]
    fn rapid_commands_start_from_the_last_submitted_frame() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.retarget(1, FROM, TO, now, Duration::from_millis(160));
        let last = motion.samples(now + Duration::from_millis(40))[0].1;
        let retargeted_at = now + Duration::from_millis(70);
        motion.retarget(1, last, FROM, retargeted_at, Duration::from_millis(160));
        assert_eq!(motion.samples(retargeted_at)[0].1, last);
        assert_eq!(motion.finish(), vec![(1, FROM)]);
    }

    #[test]
    fn cancelled_windows_and_desktops_never_receive_old_samples() {
        let now = Instant::now();
        let mut motion = Motion::default();
        for id in [1, 2] {
            motion.retarget(id, FROM, TO, now, Duration::from_millis(160));
        }
        motion.remove(1);
        assert_eq!(motion.samples(now)[0].0, 2);
        motion.clear();
        assert!(motion.samples(now + Duration::from_millis(40)).is_empty());
    }
}
