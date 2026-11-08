//! A photo's camera, lens, exposure and size, read from its file as the
//! catalog keeps them: a RAW's from its decoder's metadata, other files' from
//! their EXIF and header.
use rawmakase_model::metadata::PhotoInfo;
use std::path::Path;

/// Whether the file can be read now: an offline file, one without
/// permission or a network error is tried again later.
pub fn can_read(path: &Path) -> bool {
    use std::io::Read as _;
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut [0; 1]))
        .is_ok()
}

/// The file's info: `None` when it cannot be read now, `Some(None)` when it
/// has none. A RAW for which `header_only` holds (the app's choice: one on a
/// network volume) is read from its header alone, the whole RAW being most of
/// the cost and none of the info.
pub fn read_info(
    path: &Path,
    header_only: impl FnOnce(&Path) -> bool,
) -> Option<Option<PhotoInfo>> {
    if !can_read(path) {
        return None;
    }
    Some(if crate::storage::is_raw(path) {
        // An X3F's, which RAWmakase doesn't decode, from its header alone.
        // One with none may not have been read whole (its preview); its
        // header is read again later, which costs little.
        if let Some(undecoded) = crate::raw::undecoded(path) {
            return (undecoded.info != PhotoInfo::default()).then_some(Some(undecoded.info));
        }
        // From the header alone when asked, else, or when that can't be
        // read, from the file opened whole. A RAW that cannot be opened now
        // (still copying, a network error) is tried again later.
        let header = header_only(path)
            .then(|| crate::photo::header_metadata(path).ok())
            .flatten();
        let metadata = match header {
            Some(metadata) => metadata,
            None => crate::photo::open(path).ok()?.metadata,
        };
        Some(PhotoInfo::from_metadata(&metadata))
    } else {
        let mut info = rawmakase_interop::exif::photo_info(path).unwrap_or_default();
        // A rendered file's size as shown, after its orientation, from its
        // header alone. A header that cannot be read yet (a file still being copied) is
        // tried again later.
        info.dimensions = Some(crate::raster::dimensions(path)?);
        (info != PhotoInfo::default()).then_some(info)
    })
}
