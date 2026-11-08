//! Color Grading's wheels: where a color sits on a wheel, the color under a
//! point of it, and how History names a wheel edit. A wheel edits a region's
//! hue (0–1) and saturation (0–1), the values its sliders edit, so a grade
//! renders the same however it was set.
//!
//! Offsets are in wheel radii from the centre, x to the right and y down, as
//! on screen.

use std::f32::consts::TAU;

/// A wheel's color: hue 0–1 (red at the right, turning counter-clockwise as on
/// Lightroom's wheels) and saturation 0–1 (the centre to the rim).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HueSat {
    pub hue: f32,
    pub saturation: f32,
}

impl HueSat {
    pub const NEUTRAL: HueSat = HueSat {
        hue: 0.,
        saturation: 0.,
    };

    /// The color of a region's `[hue, saturation, luminance]`.
    pub fn of(grade: &[f32; 3]) -> Self {
        Self {
            hue: grade[0],
            saturation: grade[1],
        }
    }

    /// Writes this color into a region's `[hue, saturation, luminance]`,
    /// keeping its luminance.
    pub fn store(self, grade: &mut [f32; 3]) {
        grade[0] = self.hue;
        grade[1] = self.saturation;
    }

    /// The direction of `hue`, a wheel radius long.
    pub fn direction(hue: f32) -> [f32; 2] {
        let angle = hue * TAU;
        [angle.cos(), -angle.sin()]
    }

    /// Where the puck sits.
    pub fn offset(self) -> [f32; 2] {
        Self::direction(self.hue).map(|v| v * self.saturation)
    }

    /// The color at `offset` (clamped to the rim), in whole degrees and
    /// percent as Lightroom keeps them. At the centre, where every hue looks
    /// the same, `hue` stays.
    pub fn at(offset: [f32; 2], hue: f32) -> Self {
        let [x, y] = offset;
        let length = x.hypot(y);
        let hue = if length < 1e-4 {
            hue
        } else {
            round_hue((-y).atan2(x) / TAU)
        };
        Self {
            hue,
            saturation: round_saturation(length),
        }
    }
}

/// A hue in whole degrees, 0 to 359.
pub fn round_hue(hue: f32) -> f32 {
    (hue.rem_euclid(1.) * 360.).round() % 360. / 360.
}

/// A saturation in whole percent, 0 to 100.
pub fn round_saturation(saturation: f32) -> f32 {
    (saturation.clamp(0., 1.) * 100.).round() / 100.
}

/// History's name and value for a wheel edit from `start` to `end`: "Shadow
/// Hue", "Shadow Saturation", or both when the puck moved both ways. `region`
/// is the word Lightroom's History starts the wheel's steps with ("Shadow",
/// "Midtone", "Highlight", "Global").
pub fn wheel_step(region: &str, start: HueSat, end: HueSat) -> (String, String) {
    let hue = format!("{:.0}", end.hue * 360.);
    let saturation = format!("{:.0}", end.saturation * 100.);
    match (start.hue != end.hue, start.saturation != end.saturation) {
        (true, false) => (format!("{region} Hue"), hue),
        (false, true) => (format!("{region} Saturation"), saturation),
        _ => (
            format!("{region} Hue & Saturation"),
            format!("{hue} / {saturation}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_color_sits_where_its_hue_points_and_is_read_back_rounded() {
        let red = HueSat {
            hue: 0.,
            saturation: 1.,
        };
        let [x, y] = red.offset();
        assert!((x - 1.).abs() < 1e-6 && y.abs() < 1e-6);
        // Straight up is a quarter turn counter-clockwise.
        assert_eq!(
            HueSat::at([0., -0.5], 0.),
            HueSat {
                hue: 0.25,
                saturation: 0.5
            }
        );
        // Past the rim is the rim; hue and saturation are whole numbers.
        let blue = HueSat::at(HueSat::direction(210.4 / 360.).map(|v| v * 1.7), 0.);
        assert_eq!(
            blue,
            HueSat {
                hue: 210. / 360.,
                saturation: 1.
            }
        );
        // At the centre the hue stays.
        assert_eq!(HueSat::at([0., 0.], 0.6).hue, 0.6);
    }

    #[test]
    fn history_names_what_the_puck_moved() {
        let start = HueSat {
            hue: 0.5,
            saturation: 0.2,
        };
        let hue = HueSat { hue: 0.25, ..start };
        let saturation = HueSat {
            saturation: 0.4,
            ..start
        };
        assert_eq!(
            wheel_step("Shadow", start, hue),
            ("Shadow Hue".into(), "90".into())
        );
        assert_eq!(
            wheel_step("Global", start, saturation),
            ("Global Saturation".into(), "40".into())
        );
        assert_eq!(
            wheel_step(
                "Midtone",
                start,
                HueSat {
                    hue: 0.25,
                    saturation: 0.4
                }
            ),
            ("Midtone Hue & Saturation".into(), "90 / 40".into())
        );
    }
}
