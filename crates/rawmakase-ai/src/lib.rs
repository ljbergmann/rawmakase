//! AI Denoise's service and the installer of the models RAWmakase runs, shared by
//! the desktop app, its command line and other front ends.
//!
//! [`ai_denoise`] owns AI Denoise's queue, worker thread, cache identity and disk
//! cache; [`model_install`] downloads and verifies the model packs it and the
//! selection tools run. The models and ONNX Runtime are `rawmakase-inference`'s.
//! Neither module knows a window: the app's side of each stays in the app.

// The modules below were written inside the app crate and reach these by the
// app's paths; the same names here keep them unchanged.
use rawmakase_decode::ai::cache as ai_cache;
use rawmakase_decode::photo;
use rawmakase_engine::develop;
use rawmakase_model::{camera_data, model, storage};

pub mod ai_denoise;
pub mod model_install;
