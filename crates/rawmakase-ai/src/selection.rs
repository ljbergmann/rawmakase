//! What a Select Subject, Select Background or Select Sky run is given and what
//! the raster it makes records: the input's size, the models' identity, the
//! keys that name an input and a prompt on it, and the runtime they load with.
//! Every front end makes a selection with these, so a mask made in one reads
//! the same in another. Requests, threads and the remote worker are the
//! callers'.
use crate::storage::{FNV_OFFSET, fnv1a};
use rawmakase_inference::{LoadOptions, Prompt, RgbImage, SKY, SUBJECT};

/// The longest side of the photo rendered for the model. The network looks at
/// 1024 pixels; more only makes the stored raster larger.
pub const INPUT_EDGE: u32 = 1280;

/// Which network a job's answer comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelId {
    Subject,
    Sky,
}
impl ModelId {
    /// Its name, its weights' digest and the versions of its contract and processing.
    fn identity(self) -> (&'static str, &'static str, u32, u32) {
        match self {
            Self::Subject => (
                SUBJECT.id,
                SUBJECT.files[1].sha256,
                SUBJECT.version,
                SUBJECT.processing_version,
            ),
            Self::Sky => (SKY.id, SKY.file.sha256, SKY.version, SKY.processing_version),
        }
    }
    /// What a generated raster records as having made it.
    pub fn provenance(self) -> String {
        let (id, digest, ..) = self.identity();
        format!("{id}@{}", &digest[..16])
    }
}

/// Identifies what the model was given: the pixels, the model and the versions of
/// every step that shaped them. The same values execute the request.
pub fn input_key(rgb: &RgbImage, model: ModelId) -> String {
    let (id, digest, version, processing) = model.identity();
    let header = [
        rgb.width.to_le_bytes().as_slice(),
        rgb.height.to_le_bytes().as_slice(),
        id.as_bytes(),
        digest.as_bytes(),
        &version.to_le_bytes(),
        &processing.to_le_bytes(),
        &crate::develop::masks::SELECTION_INPUT_VERSION.to_le_bytes(),
    ]
    .concat();
    let hash = |seed| fnv1a(fnv1a(seed, &header), &rgb.data);
    format!("{:016x}{:016x}", hash(FNV_OFFSET), hash(0x84222325cbf29ce4))
}

/// What the result was made from: the photo as the model saw it (`input`, its
/// [`input_key`]) and the prompt.
pub fn prompt_key(input: &str, what: &str, prompt: &Prompt) -> String {
    let mut text = format!("{input}|{what}");
    for p in &prompt.points {
        text.push_str(&format!("|{:.4},{:.4},{}", p.x, p.y, p.positive));
    }
    if let Some(b) = prompt.bounds {
        text.push_str(&format!("|{b:.4?}"));
    }
    format!("{:016x}", fnv1a(FNV_OFFSET, text.as_bytes()))
}

/// How a selection model loads here: with the runtime installed in the runtime
/// folder when there is one, else where `rawmakase-inference` looks.
pub fn load_options() -> LoadOptions {
    let bundled = crate::model_install::runtime_dir();
    let runtime_library = rawmakase_inference::runtime::runtime_file_names()
        .iter()
        .map(|name| bundled.join(name))
        .find(|path| path.is_file());
    LoadOptions {
        runtime_library,
        threads: None,
        // Selections made on this computer run on its CPU, as before; the
        // accelerators are the remote worker's concern.
        provider: None,
        coreml: None,
    }
}

/// Whether an ONNX Runtime library this copy can load is in a place it looks.
pub fn runtime_present() -> bool {
    let names = rawmakase_inference::runtime::runtime_file_names();
    let mut dirs = rawmakase_inference::runtime::runtime_search_dirs();
    dirs.push(crate::model_install::runtime_dir());
    dirs.iter()
        .any(|dir| names.iter().any(|name| dir.join(name).is_file()))
        || std::env::var_os(rawmakase_inference::RUNTIME_ENV).is_some_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_and_subject_answers_never_share_a_key_or_a_provenance() {
        let rgb = RgbImage {
            width: 2,
            height: 1,
            data: vec![1, 2, 3, 4, 5, 6],
        };
        assert_ne!(
            input_key(&rgb, ModelId::Sky),
            input_key(&rgb, ModelId::Subject)
        );
        assert!(
            ModelId::Sky
                .provenance()
                .starts_with("upernet-convnext-base@")
        );
        assert!(
            ModelId::Subject
                .provenance()
                .starts_with("sam2.1-hiera-small@")
        );
    }
}
