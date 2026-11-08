//! Lightroom's Red Eye Correction: an ellipse over a pupil whose colour is replaced by
//! a dark neutral, with Pupil Size and Darken sliders, or for Pet Eye by black with
//! an optional catchlight. Corrections are stored as
//! parameters beside the recipe, like spot removal, and render on the linear camera
//! image before Heal and Clone, so every later edit and export sees them.
//!
//! Positions are in image space (see [`crate::model::image_frame::ImageFrame`]); sizes are
//! fractions of the photo's long edge. The ellipse is Lightroom's: semi-axes along the
//! image's x and y and their correlation (`crs:RedEyeInfo`'s `width`, `height` and
//! `alpha`), not a rotation angle. The corrections themselves are
//! [`crate::model::red_eye`]'s.
mod detect;
mod render;

use crate::camera_data::CameraImage;
use crate::model::red_eye::{EyeKind, MAX_RADIUS, RedEyeOp};

pub use detect::{DetectError, Glow, find_pupil};
pub(crate) use render::Placed;

/// What a pupil of this kind glows like, for finding it.
pub trait PupilGlow {
    /// What the pupil glows like, for finding it.
    fn glow(self) -> Glow;
}
impl PupilGlow for EyeKind {
    fn glow(self) -> Glow {
        match self {
            EyeKind::Red => Glow::Red,
            EyeKind::Pet { .. } => Glow::Bright,
        }
    }
}

/// A correction of `kind` for the pupil found within `size` (a fraction of the long
/// edge, at most [`MAX_RADIUS`]) of image-space position `center` in `im`, with Pupil
/// Size and Darken; a Pet Eye's catchlight is pulled within the pupil.
pub fn correction(
    im: &CameraImage,
    center: [f32; 2],
    size: f32,
    kind: EyeKind,
    pupil_size: f32,
    darken: f32,
) -> Result<RedEyeOp, DetectError> {
    let pupil = find_pupil(im, center, size.min(MAX_RADIUS), kind.glow())?;
    let mut op = RedEyeOp {
        kind,
        center: pupil.center,
        radius: pupil.radius,
        correlation: pupil.correlation,
        pupil_size,
        darken,
    };
    op.fit_catchlight();
    Ok(op)
}

#[cfg(test)]
mod tests;
