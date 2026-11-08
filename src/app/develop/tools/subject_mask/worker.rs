//! The thread that runs the selection model. One request at a time, one loaded
//! session reused between them, and the image embedding of the photo last asked
//! about, so a second click on it takes milliseconds instead of a second. Cancelling raises the request's flag, which the
//! runtime notices within a few milliseconds; quitting hands the thread to the shared
//! shutdown deadline rather than joining it, and the thread keeps its session until
//! it ends, so a native call is never left running on unloaded code.
use super::{Action, Done, Failure, Feature, Generated};
use crate::app::task::Stopping;
use crate::app::worker::Event;
use crate::camera_data::CameraImage;
use crate::model::masks::BitmapSource;
use crate::model::recipe::Recipe;
use crate::storage::bitmaps::Bitmap;
use crate::storage::mask_assets;
use eframe::egui;
use rawmakase_ai::selection::{INPUT_EDGE, ModelId, input_key, load_options, prompt_key};
use rawmakase_cancel::Cancel;
use rawmakase_inference::{
    Analysis, Coverage, InferenceError, Prompt, RgbImage, Session, Sky, SkySession, Subject,
};
use rawmakase_remote::{ClientError, RemoteAnalysis, RemoteSession};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender},
};
use std::time::Duration;

/// A selection to make, as the photo and edit were when it was asked for.
pub(super) struct Job {
    pub(super) load: u64,
    pub(super) generation: u64,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) image: Arc<CameraImage>,
    pub(super) recipe: Recipe,
    /// The model's folder, when the selection runs on this computer.
    pub(super) model: Option<PathBuf>,
    /// The remote worker to ask instead, when one is configured.
    pub(super) endpoint: Option<String>,
    pub(super) action: Action,
}

enum Message {
    Run(Box<Job>, Sender<Event>, egui::Context),
    /// Let go of the model, then say so.
    Unload(Sender<()>),
}

#[derive(Default)]
pub(super) struct Worker {
    messages: Option<Sender<Message>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    /// Queues `job`; its outcome arrives as an [`Event::Selection`], whatever
    /// happens, so the drawer never waits on a thread that is gone.
    pub(super) fn submit(&mut self, job: Job, tx: Sender<Event>, ctx: egui::Context) {
        let (load, generation) = (job.load, job.generation);
        let failed = |why: &str| {
            let _ = tx.send(Event::Selection(Box::new(Done {
                load,
                generation,
                result: Err(Failure::Failed(why.into())),
            })));
        };
        if self.thread.as_ref().is_some_and(|t| t.is_finished()) {
            self.messages = None;
            self.thread = None;
        }
        if self.messages.is_none() {
            let (messages, inbox) = mpsc::channel();
            match std::thread::Builder::new()
                .name("subject-select".into())
                .spawn(move || run(inbox))
            {
                Ok(thread) => {
                    self.messages = Some(messages);
                    self.thread = Some(thread);
                }
                Err(e) => return failed(&format!("could not start the selection thread: {e}")),
            }
        }
        let sent = self
            .messages
            .as_ref()
            .is_some_and(|m| m.send(Message::Run(Box::new(job), tx.clone(), ctx)).is_ok());
        if !sent {
            self.messages = None;
            failed("the selection thread stopped");
        }
    }
    /// A closure that makes the thread release the model and waits (briefly) for it
    /// to; for removing the file off the UI thread.
    pub(super) fn unloader(&self) -> impl FnOnce() + Send + 'static + use<> {
        let messages = self.messages.clone();
        move || {
            if let Some(messages) = messages {
                let (ack, done) = mpsc::channel();
                if messages.send(Message::Unload(ack)).is_ok() {
                    // A run that cannot stop leaves the removal waiting only this long.
                    let _ = done.recv_timeout(Duration::from_secs(30));
                }
            }
        }
    }
    /// Closes the mailbox and returns the thread to wait for under the shutdown
    /// deadline.
    pub(super) fn stop(&mut self) -> Stopping {
        self.messages = None;
        Stopping::new(self.thread.take())
    }
}

fn run(inbox: mpsc::Receiver<Message>) {
    let mut state = State::default();
    for message in inbox {
        match message {
            Message::Unload(ack) => {
                state = State::default();
                let _ = ack.send(());
            }
            Message::Run(job, tx, ctx) => {
                let (load, generation) = (job.load, job.generation);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    select(&job, &mut state)
                }))
                .unwrap_or_else(|_| {
                    // The session may be half-built after a panic.
                    state = State::default();
                    Err(Failure::Failed("the selection stopped unexpectedly".into()))
                });
                let _ = tx.send(Event::Selection(Box::new(Done {
                    load,
                    generation,
                    result,
                })));
                ctx.request_repaint();
            }
        }
    }
}

/// The loaded models and the embedding of the photo last selected on. Over the
/// remote endpoint, the worker's answer for the photo last asked about, so its
/// clicks are answered from what it kept. The sky of the photo last asked about, from
/// either, so finding it again costs nothing. Each model loads when first needed: the
/// sky model is large and nothing else uses it.
#[derive(Default)]
struct State {
    session: Option<Session>,
    sky_session: Option<SkySession>,
    analysis: Option<(String, Analysis)>,
    remote_analysis: Option<(String, RemoteAnalysis)>,
    sky: Option<(String, Coverage)>,
}

fn select(job: &Job, state: &mut State) -> Result<Generated, Failure> {
    let cancelled = || job.cancel.load(Ordering::Relaxed);
    if cancelled() {
        return Err(Failure::Cancelled);
    }
    let input = crate::develop::masks::selection_input(
        &job.image,
        &job.recipe,
        INPUT_EDGE,
        Cancel::new(&job.cancel),
    )
    .map_err(|e| {
        if cancelled() {
            Failure::Cancelled
        } else {
            Failure::Failed(format!("{e:#}"))
        }
    })?;
    let rgb = RgbImage {
        width: input.width as usize,
        height: input.height as usize,
        data: input.rgb8(),
    };
    let key = input_key(&rgb, job.action.model());
    let (coverage, feature, nothing, input) = match (&job.action, &job.endpoint) {
        (Action::Auto(Feature::Sky), endpoint) => select_sky(
            &rgb,
            &key,
            endpoint.as_deref(),
            job.model.as_deref(),
            Cancel::new(&job.cancel),
            state,
        )?,
        (_, Some(endpoint)) => select_remotely(endpoint, &rgb, &key, job, state)?,
        (_, None) => select_locally(&rgb, &key, job, state)?,
    };
    if cancelled() {
        return Err(Failure::Cancelled);
    }
    // Exactly nothing selected is the only emptiness decided here.
    if coverage.data.iter().all(|v| *v == 0) {
        return Err(nothing);
    }
    let (width, height) = (coverage.width as u32, coverage.height as u32);
    let id = mask_assets::register(Bitmap {
        width,
        height,
        channels: 1,
        depth: 1,
        data: coverage.data,
    })
    .map_err(|e| Failure::Failed(format!("{e:#}")))?;
    Ok(Generated {
        id,
        width,
        height,
        source: BitmapSource {
            feature: feature.provenance().into(),
            model: job.action.model().provenance(),
            input,
        },
    })
}

/// The photo's sky, from the worker when one is configured, else from the local model.
fn select_sky(
    rgb: &RgbImage,
    key: &str,
    endpoint: Option<&str>,
    model: Option<&std::path::Path>,
    cancel: Cancel<'_>,
    state: &mut State,
) -> Result<(Coverage, Feature, Failure, String), Failure> {
    if state.sky.as_ref().is_none_or(|(k, _)| *k != key) {
        state.sky = None;
        let coverage = match endpoint {
            Some(endpoint) => RemoteSession::new(endpoint)
                .sky(rgb, cancel)
                .map_err(remote_failure)?,
            None => {
                let Some(model) = model else {
                    return Err(Failure::ModelMissing);
                };
                if state.sky_session.is_none() {
                    state.sky_session =
                        Some(Sky::load_with(model, &load_options()).map_err(failure)?);
                }
                let session = state.sky_session.as_mut().expect("loaded above");
                session.select(rgb, cancel).map_err(failure)?
            }
        };
        state.sky = Some((key.to_string(), coverage));
    }
    let (_, coverage) = state.sky.as_ref().expect("found above");
    Ok((
        coverage.clone(),
        Feature::Sky,
        Failure::NoSky,
        prompt_key(key, "sky", &Prompt::default()),
    ))
}

/// What the local models make of the photo, and the prompt on it.
fn select_locally(
    rgb: &RgbImage,
    key: &str,
    job: &Job,
    state: &mut State,
) -> Result<(Coverage, Feature, Failure, String), Failure> {
    let Some(model) = &job.model else {
        return Err(Failure::ModelMissing);
    };
    if state.session.is_none() {
        state.session = Some(load(model)?);
    }
    let session = state.session.as_mut().expect("loaded above");
    if state.analysis.as_ref().is_none_or(|(k, _)| *k != key) {
        state.analysis = None;
        let analysis = session
            .analyze(rgb, Cancel::new(&job.cancel))
            .map_err(failure)?;
        state.analysis = Some((key.to_string(), analysis));
    }
    let (_, analysis) = state.analysis.as_ref().expect("analysed above");
    Ok(match &job.action {
        Action::Auto(feature) => (
            analysis.subject().map_err(failure)?,
            *feature,
            Failure::NoSubject,
            prompt_key(key, "subject", &Prompt::default()),
        ),
        Action::Click(prompt) => (
            session
                .segment(analysis.embedding(), prompt, Cancel::new(&job.cancel))
                .map_err(failure)?,
            Feature::Subject,
            Failure::NothingThere,
            prompt_key(key, "click", prompt),
        ),
    })
}

/// What the remote worker makes of the photo. The analysis it sent for this
/// photo is kept, so its clicks are answered without asking it to look again;
/// when it has forgotten the photo, it is asked once more.
fn select_remotely(
    endpoint: &str,
    rgb: &RgbImage,
    key: &str,
    job: &Job,
    state: &mut State,
) -> Result<(Coverage, Feature, Failure, String), Failure> {
    let session = RemoteSession::new(endpoint);
    // The worker keeps the photo's analysis under its own name; the one it
    // sent last is kept here, keyed the same way as a local one would be.
    let analysed = |state: &mut State| {
        if state
            .remote_analysis
            .as_ref()
            .is_none_or(|(k, _)| *k != key)
        {
            state.remote_analysis = None;
            let analysis = session
                .analyze(rgb, Cancel::new(&job.cancel))
                .map_err(remote_failure)?;
            state.remote_analysis = Some((key.to_string(), analysis));
        }
        Ok(state
            .remote_analysis
            .as_ref()
            .map(|(_, a)| a.clone())
            .expect("analysed above"))
    };
    match &job.action {
        Action::Auto(_) => {
            let analysis = analysed(state)?;
            Ok((
                analysis.subject,
                Feature::Subject,
                Failure::NoSubject,
                prompt_key(key, "subject", &Prompt::default()),
            ))
        }
        Action::Click(prompt) => {
            let analysis = analysed(state)?;
            let coverage = match session.segment(&analysis, prompt, Cancel::new(&job.cancel)) {
                Ok(coverage) => coverage,
                Err(ClientError::Expired) => {
                    // The worker keeps only the last photos: ask once more.
                    state.remote_analysis = None;
                    let analysis = analysed(state)?;
                    session
                        .segment(&analysis, prompt, Cancel::new(&job.cancel))
                        .map_err(remote_failure)?
                }
                Err(e) => return Err(remote_failure(e)),
            };
            Ok((
                coverage,
                Feature::Subject,
                Failure::NothingThere,
                prompt_key(key, "click", prompt),
            ))
        }
    }
}

/// What a remote answer means to the drawer.
fn remote_failure(error: ClientError) -> Failure {
    match error {
        ClientError::Cancelled => Failure::Cancelled,
        ClientError::Unavailable(why) => Failure::RuntimeUnavailable(why),
        ClientError::Expired => Failure::Failed("the remote worker forgot this photo".into()),
        ClientError::Failed(why) => Failure::Failed(format!("the remote worker: {why}")),
    }
}

fn load(model: &std::path::Path) -> Result<Session, Failure> {
    Subject::load_with(model, &load_options()).map_err(failure)
}

fn failure(error: InferenceError) -> Failure {
    match error {
        InferenceError::Cancelled => Failure::Cancelled,
        InferenceError::RuntimeUnavailable(why) => Failure::RuntimeUnavailable(why),
        InferenceError::ModelInvalid(why) => {
            Failure::Failed(format!("the installed model is not usable: {why}"))
        }
        InferenceError::Failed(why) | InferenceError::OutputInvalid(why) => Failure::Failed(why),
    }
}

impl Action {
    /// Sky is the sky model's; everything else, clicks included, the subject models'.
    pub(super) fn model(&self) -> ModelId {
        match self {
            Action::Auto(Feature::Sky) => ModelId::Sky,
            _ => ModelId::Subject,
        }
    }
}

/// Where selections run, when the user has said they run elsewhere: the
/// endpoint of a `rawmakase-remote-worker`, from `remote-inference.json` in
/// this computer's data folder. Absent — the usual case — means the models run
/// here, as always. See docs/remote-inference.md.
///
/// ```json
/// { "endpoint": "127.0.0.1:8765" }
/// ```
#[derive(Default)]
pub(in crate::app) struct RemoteConfig {
    /// What the file said, and when: read again at most once a second, so the
    /// drawer notices an edit without stat on every frame. Tests set this
    /// directly.
    seen: std::cell::Cell<Option<(std::time::Instant, Option<String>)>>,
}

impl RemoteConfig {
    /// The endpoint to send selections to, if there is one.
    pub(super) fn endpoint(&self) -> Option<String> {
        match self.seen.take() {
            Some((at, endpoint)) if at.elapsed() < Duration::from_secs(1) => {
                self.seen.set(Some((at, endpoint.clone())));
                endpoint
            }
            _ => {
                let endpoint = read_endpoint();
                self.seen
                    .set(Some((std::time::Instant::now(), endpoint.clone())));
                endpoint
            }
        }
    }
    /// Pretends the file said this, for the tests; the next real read wins.
    #[cfg(test)]
    pub(super) fn force(&self, endpoint: Option<&str>) {
        self.seen.set(Some((
            std::time::Instant::now(),
            endpoint.map(str::to_string),
        )));
    }
}

/// The endpoint from the configuration file, when it is a usable one.
/// `RAWMAKASE_REMOTE_ENDPOINT` says the same thing for one launch, without
/// touching the file.
fn read_endpoint() -> Option<String> {
    if let Some(endpoint) = std::env::var_os("RAWMAKASE_REMOTE_ENDPOINT")
        .map(|v| v.to_string_lossy().trim().to_string())
        .filter(|text| !text.is_empty())
    {
        return Some(endpoint);
    }
    let path = crate::storage::local_data_dir().join("remote-inference.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("endpoint")?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty() && !text.chars().any(char::is_whitespace))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels() -> RgbImage {
        RgbImage {
            width: 2,
            height: 1,
            data: vec![1, 2, 3, 4, 5, 6],
        }
    }

    #[test]
    fn only_automatic_sky_is_the_sky_models() {
        assert_eq!(Action::Auto(Feature::Sky).model(), ModelId::Sky);
        assert_eq!(Action::Auto(Feature::Subject).model(), ModelId::Subject);
        assert_eq!(Action::Auto(Feature::Background).model(), ModelId::Subject);
        assert_eq!(Action::Click(Prompt::default()).model(), ModelId::Subject);
    }

    /// The photo the same, the sky's answer is reused; another photo is asked anew. With no
    /// model and no worker, only the cached photo's answer can come back.
    #[test]
    fn a_sky_found_on_a_photo_is_reused_only_for_that_photo() {
        let rgb = pixels();
        let key = input_key(&rgb, ModelId::Sky);
        let found = Coverage {
            width: 2,
            height: 1,
            data: vec![255, 0],
        };
        let mut state = State {
            sky: Some((key.clone(), found)),
            ..State::default()
        };
        let cancel = AtomicBool::new(false);
        let (coverage, feature, ..) =
            select_sky(&rgb, &key, None, None, Cancel::new(&cancel), &mut state).unwrap();
        assert_eq!((coverage.data, feature), (vec![255, 0], Feature::Sky));
        assert!(matches!(
            select_sky(
                &rgb,
                "another",
                None,
                None,
                Cancel::new(&cancel),
                &mut state
            ),
            Err(Failure::ModelMissing)
        ));
        assert!(state.sky.is_none(), "the old photo's answer is let go of");
    }
}
