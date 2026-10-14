#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub width: f32,
    pub height: f32,
}
impl Extent {
    pub fn fit_scale(self, viewport: Self) -> f32 {
        (viewport.width / self.width.max(1.)).min(viewport.height / self.height.max(1.))
    }
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            width: self.width * scale,
            height: self.height * scale,
        }
    }
    pub fn frame_at(self, zoom: f32, maximum: Self) -> Self {
        let minimum = Self {
            width: 300.,
            height: 180.,
        };
        let upper = self.fit_scale(maximum);
        let lower = self.fit_scale(minimum).min(1.).min(upper);
        self.scaled(zoom.clamp(lower, upper))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const IMAGE: Extent = Extent {
        width: 1200.,
        height: 800.,
    };
    const SCREEN: Extent = Extent {
        width: 1500.,
        height: 900.,
    };
    #[test]
    fn frame_tracks_image_until_screen_limit_then_stops() {
        assert_eq!(
            IMAGE.frame_at(0.5, SCREEN),
            Extent {
                width: 600.,
                height: 400.
            }
        );
        assert_eq!(
            IMAGE.frame_at(1.125, SCREEN),
            Extent {
                width: 1350.,
                height: 900.
            }
        );
        assert_eq!(
            IMAGE.frame_at(2., SCREEN),
            Extent {
                width: 1350.,
                height: 900.
            }
        );
        assert_eq!(
            IMAGE.scaled(2.),
            Extent {
                width: 2400.,
                height: 1600.
            }
        );
    }
    #[test]
    fn minimum_frame_and_extreme_aspect_ratios_remain_usable() {
        assert_eq!(
            IMAGE.frame_at(0.01, SCREEN),
            Extent {
                width: 270.,
                height: 180.
            }
        );
        let panorama = Extent {
            width: 10000.,
            height: 100.,
        };
        let frame = panorama.frame_at(1., SCREEN);
        assert!((frame.width - 1500.).abs() < 0.001);
        assert!((frame.height - 15.).abs() < 0.001);
        let portrait = Extent {
            width: 100.,
            height: 10000.,
        };
        let frame = portrait.frame_at(1., SCREEN);
        assert!((frame.width - 9.).abs() < 0.001);
        assert!((frame.height - 900.).abs() < 0.001);
    }
    #[test]
    fn fit_preserves_aspect_and_matches_limiting_dimension() {
        assert!(
            (IMAGE.fit_scale(Extent {
                width: 600.,
                height: 700.
            }) - 0.5)
                .abs()
                < 0.001
        );
    }
    #[test]
    fn zooming_a_narrow_fitted_image_does_not_enlarge_its_frame() {
        let portrait = Extent {
            width: 400.,
            height: 1200.,
        };
        let frame = portrait.frame_at(0.45, SCREEN);
        assert!((frame.width - 180.).abs() < 0.001);
        assert!((frame.height - 540.).abs() < 0.001);
        let tiny = Extent {
            width: 80.,
            height: 40.,
        };
        assert_eq!(tiny.frame_at(0.9, SCREEN), tiny);
    }
}
