#[derive(Clone, Copy, Debug)]
pub struct SwipeMotion {
    pub direction: i32,
    distance: f64,
    travel: f64,
    velocity: f64,
}

impl SwipeMotion {
    pub fn new(direction: i32, width: f64) -> Self {
        Self {
            direction,
            distance: 0.,
            travel: width.max(1.) + 20.,
            velocity: 0.,
        }
    }

    pub fn update(&mut self, delta_x: f64, elapsed: f64) {
        let delta = -delta_x * self.direction as f64;
        self.distance = (self.distance + delta).clamp(0., self.travel);
        self.velocity = self.velocity * 0.6 + delta / elapsed.clamp(0.008, 0.1) * 0.4;
    }

    pub fn progress(self) -> f64 {
        self.distance / self.travel
    }

    pub fn commits(self, idle_seconds: f64) -> bool {
        let velocity = if idle_seconds < 0.12 {
            self.velocity
        } else {
            0.
        };
        if velocity < -250. && self.progress() < 0.8 {
            return false;
        }
        self.progress() >= 0.42 || (self.progress() > 0.08 && velocity > 550.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pauses_hold_position_and_release_uses_position() {
        let mut swipe = SwipeMotion::new(1, 580.);
        swipe.update(-180., 0.1);
        assert_eq!(swipe.progress(), 0.3);
        assert!(!swipe.commits(1.));
        swipe.update(-120., 0.1);
        assert_eq!(swipe.progress(), 0.5);
        assert!(swipe.commits(1.));
    }
    #[test]
    fn reversing_and_flicking_work_in_both_directions() {
        for direction in [-1, 1] {
            let mut swipe = SwipeMotion::new(direction, 580.);
            swipe.update(-100. * direction as f64, 0.016);
            assert!(swipe.commits(0.01));
            swipe.update(150. * direction as f64, 0.016);
            assert_eq!(swipe.progress(), 0.);
            assert!(!swipe.commits(0.01));
            swipe.update(-10000. * direction as f64, 0.1);
            assert_eq!(swipe.progress(), 1.);
        }
    }
}
