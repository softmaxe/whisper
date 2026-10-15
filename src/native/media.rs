//! Pauses playing media when Dictation starts and resumes it afterwards.
//!
//! MediaRemote is closed to unprivileged processes on macOS 15.4+, so the
//! state-aware path loads the vendored MediaRemoteAdapter framework through
//! `/usr/bin/perl` (`get` to see whether anything plays, `send 1`/`send 0` to
//! pause/play). Without it, `macos-media-remote --media-key-toggle` posts a
//! play/pause media key, which cannot know whether anything was playing.
//!
//! Pause, resume and toggle run one at a time: a quick tap could otherwise
//! run the resume before the pause recorded what it paused. All calls block
//! (helper deadlines of 3 s); call them off the main thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::paths;

use super::helper::{RunOutput, resolve_helper, run_with_timeout};
use super::timer::lock;

const HELPER_TIMEOUT: Duration = Duration::from_secs(3);
const PERL: &str = "/usr/bin/perl";
const ADAPTER_SCRIPT: &str = "mediaremote-adapter.pl";
const ADAPTER_FRAMEWORK: &str = "MediaRemoteAdapter.framework";
const MEDIA_KEY_BINARY: &str = "macos-media-remote";
/// kMRAPause / kMRAPlay.
const SEND_PAUSE: &str = "1";
const SEND_PLAY: &str = "0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterPaths {
    pub perl: PathBuf,
    pub script: PathBuf,
    pub framework: PathBuf,
}

pub type Runner = Arc<dyn Fn(&Path, &[&str]) -> RunOutput + Send + Sync>;

#[derive(Clone)]
pub struct MediaHooks {
    pub run: Runner,
    pub adapter: Arc<dyn Fn() -> Option<AdapterPaths> + Send + Sync>,
    pub media_key_binary: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>,
}

impl Default for MediaHooks {
    fn default() -> Self {
        Self {
            run: Arc::new(|program, args| run_with_timeout(program, args, None, HELPER_TIMEOUT)),
            adapter: Arc::new(resolve_adapter),
            media_key_binary: Arc::new(|| resolve_helper(MEDIA_KEY_BINARY)),
        }
    }
}

/// The adapter script ships next to the helpers in a bundle and under
/// `resources/mediaremote-adapter/bin` in development.
pub fn resolve_adapter() -> Option<AdapterPaths> {
    let perl = PathBuf::from(PERL);
    if !perl.exists() {
        return None;
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    let scripts = [
        paths::helper(ADAPTER_SCRIPT),
        manifest
            .join("mediaremote-adapter")
            .join("bin")
            .join(ADAPTER_SCRIPT),
    ];
    let frameworks = [
        paths::helper(ADAPTER_FRAMEWORK),
        manifest.join("bin").join(ADAPTER_FRAMEWORK),
    ];
    let script = scripts.into_iter().find(|p| p.exists())?;
    let framework = frameworks.into_iter().find(|p| p.exists())?;
    Some(AdapterPaths {
        perl,
        script,
        framework,
    })
}

#[derive(Default)]
struct State {
    did_pause: bool,
    paused_via_adapter: bool,
}

pub struct MediaPlayer {
    hooks: MediaHooks,
    // Held for a whole operation, serializing pause/resume/toggle.
    state: Mutex<State>,
}

impl MediaPlayer {
    pub fn new(hooks: MediaHooks) -> Self {
        Self {
            hooks,
            state: Mutex::new(State::default()),
        }
    }

    pub fn shared() -> &'static MediaPlayer {
        static SHARED: OnceLock<MediaPlayer> = OnceLock::new();
        SHARED.get_or_init(|| MediaPlayer::new(MediaHooks::default()))
    }

    /// Pauses playing media. True if a pause was sent.
    pub fn pause_media(&self) -> bool {
        let mut state = lock(&self.state);
        self.pause_locked(&mut state)
    }

    /// Resumes what [`MediaPlayer::pause_media`] paused. True if resumed.
    pub fn resume_media(&self) -> bool {
        let mut state = lock(&self.state);
        self.resume_locked(&mut state)
    }

    fn run_adapter(&self, args: &[&str]) -> Option<RunOutput> {
        let paths = (self.hooks.adapter)()?;
        let script = paths.script.display().to_string();
        let framework = paths.framework.display().to_string();
        let mut full: Vec<&str> = vec![&script, &framework];
        full.extend_from_slice(args);
        Some((self.hooks.run)(&paths.perl, &full))
    }

    fn pause_locked(&self, state: &mut State) -> bool {
        state.did_pause = false;
        state.paused_via_adapter = false;

        if let Some(probe) = self.run_adapter(&["get", "--no-artwork"]) {
            if probe.success() {
                let output = probe.stdout.trim();
                let playing = if output == "null" {
                    Some(false)
                } else if output.is_empty() {
                    None
                } else {
                    serde_json::from_str::<serde_json::Value>(output)
                        .ok()
                        .map(|value| {
                            value
                                .get("playing")
                                .is_some_and(|p| p.as_bool() == Some(true))
                        })
                };
                match playing {
                    Some(false) => {
                        log::debug!("[media] adapter reports nothing playing");
                        return false;
                    }
                    Some(true) => {
                        let pause = self.run_adapter(&["send", SEND_PAUSE]);
                        if pause.as_ref().is_some_and(RunOutput::success) {
                            log::debug!("[media] paused via adapter");
                            state.paused_via_adapter = true;
                            state.did_pause = true;
                            return true;
                        }
                        log::debug!("[media] adapter pause failed: {pause:?}");
                    }
                    None => {}
                }
            } else {
                log::debug!(
                    "[media] adapter get failed ({:?}), falling back to media key",
                    probe.status
                );
            }
        }

        // Fallback: a media key toggle; it cannot know whether anything plays.
        if self.send_media_key() {
            state.did_pause = true;
            return true;
        }
        false
    }

    fn resume_locked(&self, state: &mut State) -> bool {
        if !state.did_pause {
            return false;
        }
        let used_adapter = state.paused_via_adapter;
        state.did_pause = false;
        state.paused_via_adapter = false;
        if used_adapter {
            let play = self.run_adapter(&["send", SEND_PLAY]);
            if play.as_ref().is_some_and(RunOutput::success) {
                log::debug!("[media] resumed via adapter");
                return true;
            }
            log::debug!("[media] adapter play failed, falling back to media key");
        }
        self.send_media_key()
    }

    fn send_media_key(&self) -> bool {
        let Some(binary) = (self.hooks.media_key_binary)() else {
            return false;
        };
        let result = (self.hooks.run)(&binary, &["--media-key-toggle"]);
        if !result.success() {
            log::debug!("[media] media key helper failed: {:?}", result.status);
        }
        result.success()
    }
}

/// Pauses media via the shared player.
pub fn pause_media() -> bool {
    MediaPlayer::shared().pause_media()
}

/// Resumes media paused by [`pause_media`].
pub fn resume_media() -> bool {
    MediaPlayer::shared().resume_media()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    type Call = (String, Vec<String>);

    struct Fixture {
        player: Arc<MediaPlayer>,
        calls: Arc<Mutex<Vec<Call>>>,
        // Each helper run blocks until the test sends its result.
        results: mpsc::Sender<RunOutput>,
    }

    fn output(status: i32, stdout: &str) -> RunOutput {
        RunOutput {
            status: Some(status),
            stdout: stdout.into(),
            ..Default::default()
        }
    }

    fn fixture(adapter: bool, media_key: bool) -> Fixture {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel::<RunOutput>();
        let rx = Mutex::new(rx);
        let recorded = Arc::clone(&calls);
        let player = Arc::new(MediaPlayer::new(MediaHooks {
            run: Arc::new(move |program, args| {
                let name = program
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                lock(&recorded).push((name, args.iter().map(|a| a.to_string()).collect()));
                lock(&rx).recv().unwrap_or_default()
            }),
            adapter: Arc::new(move || {
                adapter.then(|| AdapterPaths {
                    perl: PathBuf::from("/usr/bin/perl"),
                    script: PathBuf::from("/r/mediaremote-adapter.pl"),
                    framework: PathBuf::from("/r/MediaRemoteAdapter.framework"),
                })
            }),
            media_key_binary: Arc::new(move || {
                media_key.then(|| PathBuf::from("/r/macos-media-remote"))
            }),
        }));
        Fixture {
            player,
            calls,
            results: tx,
        }
    }

    fn wait_for_call(calls: &Mutex<Vec<Call>>, index: usize) -> Call {
        for _ in 0..2000 {
            if let Some(call) = lock(calls).get(index) {
                return call.clone();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("expected call #{}", index + 1);
    }

    fn tail(call: &Call) -> Vec<String> {
        call.1[call.1.len().saturating_sub(2)..].to_vec()
    }

    #[test]
    fn resume_waits_for_an_in_flight_pause() {
        let f = fixture(true, true);
        let p = Arc::clone(&f.player);
        let pausing = std::thread::spawn(move || p.pause_media());
        let probe = wait_for_call(&f.calls, 0);
        let r = Arc::clone(&f.player);
        let resuming = std::thread::spawn(move || r.resume_media());
        assert_eq!(probe.0, "perl");
        assert_eq!(tail(&probe), ["get", "--no-artwork"]);
        f.results.send(output(0, "{\"playing\":true}")).unwrap();

        let pause = wait_for_call(&f.calls, 1);
        assert_eq!(tail(&pause), ["send", "1"]);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(lock(&f.calls).len(), 2, "resume must wait for the pause");
        f.results.send(output(0, "")).unwrap();
        assert!(pausing.join().unwrap());

        let play = wait_for_call(&f.calls, 2);
        assert_eq!(tail(&play), ["send", "0"]);
        f.results.send(output(0, "")).unwrap();
        assert!(resuming.join().unwrap());
        assert!(!lock(&f.player.state).did_pause);
    }

    #[test]
    fn nothing_playing_sends_nothing_and_resume_is_a_no_op() {
        let f = fixture(true, true);
        f.results.send(output(0, "null")).unwrap();
        assert!(!f.player.pause_media());
        assert!(!f.player.resume_media());
        assert_eq!(lock(&f.calls).len(), 1);
    }

    #[test]
    fn adapter_failure_falls_back_to_the_media_key_both_ways() {
        let f = fixture(true, true);
        f.results.send(output(1, "")).unwrap();
        f.results.send(output(0, "OK")).unwrap();
        assert!(f.player.pause_media());
        assert_eq!(lock(&f.calls)[1].1, ["--media-key-toggle"]);
        f.results.send(output(0, "OK")).unwrap();
        assert!(f.player.resume_media());
        assert_eq!(lock(&f.calls)[2].0, "macos-media-remote");
    }

    #[test]
    fn without_any_helper_nothing_happens() {
        let f = fixture(false, false);
        assert!(!f.player.pause_media());
        assert!(!f.player.resume_media());
        assert!(lock(&f.calls).is_empty());
    }

    #[test]
    #[ignore = "talks to the real MediaRemote adapter"]
    fn real_adapter_probe() {
        let paths = resolve_adapter().expect("adapter built");
        let output = run_with_timeout(
            &paths.perl,
            &[
                &paths.script.display().to_string(),
                &paths.framework.display().to_string(),
                "get",
                "--no-artwork",
            ],
            None,
            HELPER_TIMEOUT,
        );
        assert!(output.success(), "{output:?}");
    }
}
