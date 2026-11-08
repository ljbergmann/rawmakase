//! The desktop's side of importing profiles, lens profiles and presets
//! ([`crate::bulk_import`]): starting an import in the background, and what
//! the editor does once it ends.
use crate::app::{Editor, worker::Event};
pub use crate::bulk_import::import_profiles;
pub(crate) use crate::bulk_import::{
    ImportKind, ImportProgress, LIGHTROOM_PRESETS, Summary, find_files, run,
};
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

impl Editor {
    /// Starts importing `picks` in the background; the status line follows it.
    pub(in crate::app) fn import(
        &mut self,
        kind: ImportKind,
        picks: Vec<PathBuf>,
        ctx: &egui::Context,
    ) {
        if self.importing.is_some() || picks.is_empty() {
            return;
        }
        // A profile download publishes into the same library: profiles, looks and
        // presets (which bring their looks) wait for it, said so rather than
        // dropped silently. Lens profiles don't touch it.
        if !matches!(kind, ImportKind::LensProfiles)
            && let Some(name) = self.profile_download.downloading()
        {
            self.messages.warn(format!(
                "Not imported: {name} is downloading into the profile library. Import again when it finishes."
            ));
            return;
        }
        let progress = Arc::new(ImportProgress::new({
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        }));
        self.importing = Some(progress.clone());
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        let first = picks[0].clone();
        crate::app::task::spawn(
            tx,
            ctx,
            move |tx| {
                let summary = run(kind, &picks, &progress);
                let _ = tx.send(Event::Imported(Box::new(summary)));
            },
            move |tx, error| {
                let summary = Summary {
                    failed: vec![(first, error)],
                    ..Summary::new(kind)
                };
                let _ = tx.send(Event::Imported(Box::new(summary)));
            },
        );
    }
    pub(in crate::app) fn imported(&mut self, summary: Summary, ctx: &egui::Context) {
        self.importing = None;
        // A preset import takes the looks among its files too.
        let profiles = match summary.kind {
            ImportKind::CameraProfiles | ImportKind::Luts(_) => summary.imported,
            ImportKind::Presets => summary.profiles,
            ImportKind::LensProfiles => 0,
        };
        if profiles > 0 {
            self.list_profiles_after_import();
        }
        if summary.imported > 0 {
            match summary.kind {
                ImportKind::CameraProfiles | ImportKind::Luts(_) => {}
                ImportKind::LensProfiles => {
                    // Lens profiles are matched when a photo opens: reopen it.
                    if let Some(path) = self.document.path.clone() {
                        let photo = self.document.catalog_photo;
                        self.open_raw(path, photo);
                    }
                    // And the reference photo, whose lens may now have a profile.
                    self.reload_reference();
                }
                ImportKind::Presets => self.reload_presets(ctx),
            }
        }
        let mut said = summary.message();
        if summary.has_details() {
            said.push_str(": ");
            said.push_str(&summary.details(1));
        }
        if summary.failures() > 0 && summary.done() == 0 {
            self.messages.fail(said);
        } else if summary.failures() > 0 || summary.done() == 0 {
            self.messages.warn(said);
        } else {
            self.messages.done(said);
        }
        self.onboarding.imported(summary);
    }
    /// Lists the open photo's profiles again once an import has added some.
    pub(in crate::app) fn list_profiles_after_import(&mut self) {
        self.profile_library = crate::camera_profiles::library_fingerprint();
        let before = self.document.edit.recipe().clone();
        if let Some(m) = &self.document.metadata {
            let listed = crate::camera_profiles::listed(m);
            self.document.profiles = crate::app::develop::camera_look::keep_fitted_default(
                &self.document.profiles,
                m,
                listed.profiles,
            );
            self.document.profile_errors = listed.errors;
            self.document.profile_groups = listed.groups;
            // Importing never changes an edit: the user picks a profile. A photo
            // without one follows the raw defaults, which may now resolve to an
            // imported profile.
            self.refresh_photo_defaults();
            self.refresh_preset_support();
        }
        let current = self.unchanged_current_photo(&before);
        self.refresh_library_defaults_preserving(current);
    }
}
