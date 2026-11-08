//! Lightroom's crop guide overlays (Grid, Thirds, Diagonal, Triangle, Golden
//! Ratio and Golden Spiral) and where their lines fall on a crop, for any
//! interface to draw.

/// Lightroom's crop guide overlays, in the order O cycles them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Guide {
    Grid,
    #[default]
    Thirds,
    Diagonal,
    Triangle,
    GoldenRatio,
    GoldenSpiral,
}
impl Guide {
    pub const ALL: [Guide; 6] = [
        Guide::Grid,
        Guide::Thirds,
        Guide::Diagonal,
        Guide::Triangle,
        Guide::GoldenRatio,
        Guide::GoldenSpiral,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Guide::Grid => "Grid",
            Guide::Thirds => "Thirds",
            Guide::Diagonal => "Diagonal",
            Guide::Triangle => "Triangle",
            Guide::GoldenRatio => "Golden Ratio",
            Guide::GoldenSpiral => "Golden Spiral",
        }
    }
    /// Its name in the session file, which stays the same across versions.
    pub fn key(self) -> &'static str {
        match self {
            Guide::Grid => "grid",
            Guide::Thirds => "thirds",
            Guide::Diagonal => "diagonal",
            Guide::Triangle => "triangle",
            Guide::GoldenRatio => "golden-ratio",
            Guide::GoldenSpiral => "golden-spiral",
        }
    }
    /// How many ways round Shift+O can turn it: the triangle's diagonal runs either way,
    /// and the spiral can close on any corner.
    pub fn orientations(self) -> u8 {
        match self {
            Guide::Triangle => 2,
            Guide::GoldenSpiral => 4,
            _ => 1,
        }
    }
}

impl Guide {
    /// The overlay's lines over a crop `w` × `h` across, from its top-left
    /// corner, turned `orientation` ways (below [`Self::orientations`]):
    /// each a polyline.
    pub fn lines(self, orientation: u8, w: f32, h: f32) -> Vec<Vec<[f32; 2]>> {
        let p = |x: f32, y: f32| [x, y];
        let across = |t: f32| vec![p(w * t, 0.), p(w * t, h)];
        let down = |t: f32| vec![p(0., h * t), p(w, h * t)];
        let mut lines = match self {
            Guide::Grid => {
                // Squares about a twelfth of the long edge.
                let step = w.max(h) / 12.;
                let mut lines = Vec::new();
                let mut x = step;
                while x < w - 0.5 {
                    lines.push(vec![p(x, 0.), p(x, h)]);
                    x += step;
                }
                let mut y = step;
                while y < h - 0.5 {
                    lines.push(vec![p(0., y), p(w, y)]);
                    y += step;
                }
                lines
            }
            Guide::Thirds => vec![
                across(1. / 3.),
                across(2. / 3.),
                down(1. / 3.),
                down(2. / 3.),
            ],
            Guide::GoldenRatio => {
                let t = 1. / std::f32::consts::GOLDEN_RATIO;
                vec![across(1. - t), across(t), down(1. - t), down(t)]
            }
            Guide::Diagonal => {
                // At 45° from each corner, as far as the opposite long edge.
                let d = w.min(h);
                vec![
                    vec![p(0., 0.), p(d, d)],
                    vec![p(w, 0.), p(w - d, d)],
                    vec![p(0., h), p(d, h - d)],
                    vec![p(w, h), p(w - d, h - d)],
                ]
            }
            Guide::Triangle => {
                // The diagonal, and from the other two corners the lines that meet it
                // at right angles.
                let foot = |c: [f32; 2]| {
                    let t = (c[0] * w + c[1] * h) / (w * w + h * h);
                    p(t * w, t * h)
                };
                vec![
                    vec![p(0., 0.), p(w, h)],
                    vec![p(w, 0.), foot(p(w, 0.))],
                    vec![p(0., h), foot(p(0., h))],
                ]
            }
            Guide::GoldenSpiral => golden_spiral(w, h),
        };
        // Odd orientations mirror across; the spiral's 2 and 3 also mirror down.
        let mirror_x = orientation % 2 == 1;
        let mirror_y = orientation >= 2;
        for line in &mut lines {
            for q in line.iter_mut() {
                if mirror_x {
                    q[0] = w - q[0];
                }
                if mirror_y {
                    q[1] = h - q[1];
                }
            }
        }
        lines
    }
}

/// The golden spiral over a crop `size` across: squares cut from golden rectangles,
/// each with a quarter circle, closing on the lower right, stretched to the crop as
/// Lightroom's is.
fn golden_spiral(width: f32, height: f32) -> Vec<Vec<[f32; 2]>> {
    let phi = std::f32::consts::GOLDEN_RATIO;
    let (sx, sy) = (width / phi, height);
    let at = |x: f32, y: f32| [x * sx, y * sy];
    let mut lines = Vec::new();
    // The rectangle left to cut: left, top, width, height in a φ × 1 frame.
    let (mut x, mut y, mut w, mut h) = (0f32, 0f32, phi, 1f32);
    for i in 0..10 {
        // Square side, its quarter circle's centre and the circle's start and end angles
        // (y down), and the divider left behind.
        let (s, centre, from, divider, rest) = match i % 4 {
            0 => (
                h,
                (x + h, y + h),
                180f32,
                [(x + h, y), (x + h, y + h)],
                (x + h, y, w - h, h),
            ),
            1 => (
                w,
                (x, y + w),
                270.,
                [(x, y + w), (x + w, y + w)],
                (x, y + w, w, h - w),
            ),
            2 => (
                h,
                (x + w - h, y),
                0.,
                [(x + w - h, y), (x + w - h, y + h)],
                (x, y, w - h, h),
            ),
            _ => (
                w,
                (x + w, y + h - w),
                90.,
                [(x, y + h - w), (x + w, y + h - w)],
                (x, y, w, h - w),
            ),
        };
        let arc = (0..=12)
            .map(|k| {
                let a = (from + 90. * k as f32 / 12.).to_radians();
                at(centre.0 + s * a.cos(), centre.1 + s * a.sin())
            })
            .collect();
        lines.push(arc);
        lines.push(vec![
            at(divider[0].0, divider[0].1),
            at(divider[1].0, divider[1].1),
        ]);
        (x, y, w, h) = rest;
    }
    lines
}
