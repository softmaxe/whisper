//! Automatic paste into the Target app, driving `macos-fast-paste`.
//!
//! Sequence: activate the Target app, append
//! smart spacing, snapshot the clipboard, write the transcript, check
//! Accessibility, confirm the target accepts a paste, wait 120 ms, post
//! Command-V through the helper (osascript as fallback), and 450 ms later put
//! the previous clipboard back unless something else replaced it. Pastes are
//! queued so one paste's restore never interleaves with the next paste.
//!
//! `paste_text` blocks (up to a few seconds); call it off the main thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypePNG, NSPasteboardTypeRTF,
    NSPasteboardTypeString, NSPasteboardTypeTIFF,
};
use objc2_foundation::{NSArray, NSData, NSString};

use super::helper::{RunOutput, resolve_helper, run_with_timeout};
use super::permissions::{SettingsPane, open_settings};
use super::text_monitor::TextEditMonitor;
use super::timer::lock;

pub use super::permissions::is_accessibility_trusted;

const PASTE_DELAY: Duration = Duration::from_millis(120);
const RESTORE_DELAY: Duration = Duration::from_millis(450);
const RETRY_DELAY: Duration = Duration::from_millis(200);
const PASTE_TIMEOUT: Duration = Duration::from_secs(3);
/// Debounces the permission dialog shown on denial.
const ACCESSIBILITY_CHECK_TTL: Duration = Duration::from_secs(5);
const FAST_PASTE_BINARY: &str = "macos-fast-paste";
const OSASCRIPT: &str = "/usr/bin/osascript";
const OSASCRIPT_PASTE: [&str; 2] = [
    "-e",
    "tell application \"System Events\" to key code 9 using command down",
];

// ---------------------------------------------------------------------------
// Smart spacing

/// Appends a trailing space so the next dictation does not run into this
/// one, except after whitespace or unspaced scripts (Han, kana, CJK
/// punctuation). Hangul keeps the space: Korean separates words.
pub fn apply_smart_spacing(text: &str) -> String {
    let Some(last) = text.chars().last() else {
        return String::new();
    };
    if last.is_whitespace() || ends_with_cjk(last) {
        return text.to_string();
    }
    format!("{text} ")
}

fn ends_with_cjk(c: char) -> bool {
    static CJK: OnceLock<Option<regex::Regex>> = OnceLock::new();
    let cjk = CJK.get_or_init(|| {
        regex::Regex::new(
            r"[\p{Han}\p{Hiragana}\p{Katakana}\u{3000}-\u{303f}\u{ff00}-\u{ff65}\u{fe10}-\u{fe1f}\u{fe30}-\u{fe4f}]",
        )
        .ok()
    });
    let mut buffer = [0u8; 4];
    cjk.as_ref()
        .is_some_and(|cjk| cjk.is_match(c.encode_utf8(&mut buffer)))
}

// ---------------------------------------------------------------------------
// Clipboard

/// The clipboard formats preserved across a paste: text, HTML, RTF and image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    /// (pasteboard type, data) pairs in their original order.
    pub items: Vec<(String, Vec<u8>)>,
}

impl ClipboardSnapshot {
    #[cfg(test)]
    pub fn text(&self) -> Option<String> {
        self.items
            .iter()
            .find(|(kind, _)| kind == "public.utf8-plain-text")
            .map(|(_, data)| String::from_utf8_lossy(data).into_owned())
    }
}

pub trait Pasteboard: Send + Sync {
    fn read_text(&self) -> Option<String>;
    fn write_text(&self, text: &str) -> bool;
    fn snapshot(&self) -> ClipboardSnapshot;
    fn restore(&self, snapshot: &ClipboardSnapshot);
}

/// The general NSPasteboard.
pub struct SystemPasteboard;

fn general() -> Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

fn preserved_types() -> [&'static NSString; 5] {
    // SAFETY: AppKit's pasteboard type constants are immutable NSStrings.
    unsafe {
        [
            NSPasteboardTypeString,
            NSPasteboardTypeHTML,
            NSPasteboardTypeRTF,
            NSPasteboardTypePNG,
            NSPasteboardTypeTIFF,
        ]
    }
}

impl Pasteboard for SystemPasteboard {
    fn read_text(&self) -> Option<String> {
        // SAFETY: see `preserved_types`.
        let kind = unsafe { NSPasteboardTypeString };
        general().stringForType(kind).map(|s| s.to_string())
    }

    fn write_text(&self, text: &str) -> bool {
        let pasteboard = general();
        pasteboard.clearContents();
        // SAFETY: see `preserved_types`.
        let kind = unsafe { NSPasteboardTypeString };
        pasteboard.setString_forType(&NSString::from_str(text), kind)
    }

    fn snapshot(&self) -> ClipboardSnapshot {
        let pasteboard = general();
        let mut items = Vec::new();
        for kind in preserved_types() {
            if let Some(data) = pasteboard.dataForType(kind)
                && data.length() > 0
            {
                items.push((kind.to_string(), data.to_vec()));
            }
        }
        ClipboardSnapshot { items }
    }

    fn restore(&self, snapshot: &ClipboardSnapshot) {
        let pasteboard = general();
        pasteboard.clearContents();
        if snapshot.items.is_empty() {
            return;
        }
        let kinds: Vec<Retained<NSString>> = snapshot
            .items
            .iter()
            .map(|(kind, _)| NSString::from_str(kind))
            .collect();
        let refs: Vec<&NSString> = kinds.iter().map(|k| &**k).collect();
        let array = NSArray::from_slice(&refs);
        // SAFETY: declareTypes:owner: accepts a nil owner.
        unsafe {
            pasteboard.declareTypes_owner(&array, None);
        }
        for ((_, data), kind) in snapshot.items.iter().zip(&kinds) {
            pasteboard.setData_forType(Some(&NSData::with_bytes(data)), kind);
        }
        log::debug!("[paste] clipboard restored");
    }
}

/// Replaces the clipboard with `text`.
pub fn write_clipboard(text: &str) -> bool {
    SystemPasteboard.write_text(text)
}

// ---------------------------------------------------------------------------
// Paste

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteOptions {
    /// The Dictation's Target app (captured at the hotkey press).
    pub target_pid: Option<i32>,
    /// Put the previous clipboard back after pasting.
    pub restore_clipboard: bool,
    /// Without Accessibility, leave the text on the clipboard instead of
    /// failing (and do not show the permission dialog).
    pub allow_clipboard_fallback: bool,
}

impl Default for PasteOptions {
    fn default() -> Self {
        Self {
            target_pid: None,
            restore_clipboard: true,
            allow_clipboard_fallback: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteFailure {
    AccessibilityRequired,
    KeyboardLayoutUnresolved,
    TimedOut,
    CommandFailed(String),
}

/// `pasted: false` with no failure means the text was left on the clipboard
/// for a manual paste (unconfirmed target or clipboard fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteOutcome {
    pub pasted: bool,
    pub failure: Option<PasteFailure>,
}

impl PasteOutcome {
    fn clipboard_only() -> Self {
        Self {
            pasted: false,
            failure: None,
        }
    }
}

/// Runs a paste command (`program args`) to completion with a deadline.
pub type CommandRunner = Arc<dyn Fn(&Path, &[&str], Duration) -> RunOutput + Send + Sync>;

/// Seams for tests; production uses AppKit, the helpers and the monitor.
#[derive(Clone)]
pub struct PasteHooks {
    pub pasteboard: Arc<dyn Pasteboard>,
    pub run: CommandRunner,
    pub resolve_fast_paste: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    pub accessibility_trusted: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Shows the "grant Accessibility" dialog (non-silent denial).
    pub show_accessibility_dialog: Arc<dyn Fn() + Send + Sync>,
    pub activate_target: Arc<dyn Fn(Option<i32>) -> bool + Send + Sync>,
    /// Paste-target verdict for the pid (see `TextEditMonitor`).
    pub check_paste_target: Arc<dyn Fn(Option<i32>) -> Option<bool> + Send + Sync>,
    pub paste_delay: Duration,
    pub restore_delay: Duration,
    pub retry_delay: Duration,
}

impl Default for PasteHooks {
    fn default() -> Self {
        Self {
            pasteboard: Arc::new(SystemPasteboard),
            run: Arc::new(|program, args, timeout| run_with_timeout(program, args, None, timeout)),
            resolve_fast_paste: Arc::new(|| resolve_helper(FAST_PASTE_BINARY)),
            accessibility_trusted: Arc::new(|| is_accessibility_trusted(false)),
            show_accessibility_dialog: Arc::new(show_accessibility_dialog),
            activate_target: Arc::new(|pid| TextEditMonitor::shared().activate_pid(pid)),
            check_paste_target: Arc::new(|pid| TextEditMonitor::shared().can_paste_at_target(pid)),
            paste_delay: PASTE_DELAY,
            restore_delay: RESTORE_DELAY,
            retry_delay: RETRY_DELAY,
        }
    }
}

/// One paste at a time, including its delayed clipboard restore.
#[derive(Default)]
struct Queue {
    busy: Mutex<bool>,
    idle: Condvar,
}

struct Ticket(Arc<Queue>);

impl Queue {
    fn acquire(self: &Arc<Self>) -> Ticket {
        let mut busy = lock(&self.busy);
        while *busy {
            busy = self
                .idle
                .wait(busy)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        *busy = true;
        Ticket(Arc::clone(self))
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        *lock(&self.0.busy) = false;
        self.0.idle.notify_one();
    }
}

#[derive(Default)]
struct Caches {
    fast_paste_disabled: bool,
    accessibility: Option<(bool, Instant)>,
}

/// Clipboard paste engine.
pub struct Paster {
    hooks: PasteHooks,
    queue: Arc<Queue>,
    caches: Mutex<Caches>,
}

struct PastedRestore {
    original: ClipboardSnapshot,
    expected: String,
}

impl Paster {
    pub fn new(hooks: PasteHooks) -> Self {
        Self {
            hooks,
            queue: Arc::new(Queue::default()),
            caches: Mutex::new(Caches::default()),
        }
    }

    pub fn shared() -> &'static Paster {
        static SHARED: OnceLock<Paster> = OnceLock::new();
        SHARED.get_or_init(|| Paster::new(PasteHooks::default()))
    }

    /// Activates the Target app and pastes `text` (with smart spacing) into it.
    pub fn paste_text(&self, text: &str, options: PasteOptions) -> PasteOutcome {
        // Activation stays outside the clipboard queue.
        if options.target_pid.is_some() {
            (self.hooks.activate_target)(options.target_pid);
        }
        let spaced = apply_smart_spacing(text);
        let ticket = self.queue.acquire();
        let (outcome, restore) = self.paste_locked(&spaced, options);
        match restore {
            // The ticket is held until the restore settles.
            Some(restore) => {
                let pasteboard = Arc::clone(&self.hooks.pasteboard);
                let delay = self.hooks.restore_delay;
                std::thread::spawn(move || {
                    restore_after_delay(
                        pasteboard.as_ref(),
                        &restore.original,
                        delay,
                        &restore.expected,
                    );
                    drop(ticket);
                });
            }
            None => drop(ticket),
        }
        outcome
    }

    /// Runs `operation` inside the paste queue (after pending restores).
    #[cfg(test)]
    pub fn run_clipboard_operation<T>(&self, operation: impl FnOnce() -> T) -> T {
        let _ticket = self.queue.acquire();
        operation()
    }

    fn paste_locked(
        &self,
        text: &str,
        options: PasteOptions,
    ) -> (PasteOutcome, Option<PastedRestore>) {
        let pasteboard = &self.hooks.pasteboard;
        let original = options.restore_clipboard.then(|| pasteboard.snapshot());
        pasteboard.write_text(text);

        if !self.check_accessibility(options.allow_clipboard_fallback) {
            log::info!("[paste] no Accessibility permission, text left on the clipboard");
            if options.allow_clipboard_fallback {
                return (PasteOutcome::clipboard_only(), None);
            }
            return (
                PasteOutcome {
                    pasted: false,
                    failure: Some(PasteFailure::AccessibilityRequired),
                },
                None,
            );
        }

        // Probe after writing the transcript so the target validates its
        // Paste menu against this clipboard. Only a confirmed target pastes.
        if (self.hooks.check_paste_target)(options.target_pid) != Some(true) {
            log::info!("[paste] paste target not confirmed, keeping text in clipboard");
            return (PasteOutcome::clipboard_only(), None);
        }

        let result = match self.paste_command() {
            Ok(()) => Ok(()),
            Err(PasteFailure::KeyboardLayoutUnresolved) => {
                Err(PasteFailure::KeyboardLayoutUnresolved)
            }
            Err(first) => {
                log::warn!("[paste] first attempt failed ({first:?}), retrying");
                pasteboard.write_text(text);
                std::thread::sleep(self.hooks.retry_delay);
                self.paste_command()
            }
        };
        match result {
            Ok(()) => (
                PasteOutcome {
                    pasted: true,
                    failure: None,
                },
                original.map(|original| PastedRestore {
                    original,
                    expected: text.to_string(),
                }),
            ),
            Err(failure) => (
                PasteOutcome {
                    pasted: false,
                    failure: Some(failure),
                },
                None,
            ),
        }
    }

    fn fast_paste_binary(&self) -> Option<PathBuf> {
        if lock(&self.caches).fast_paste_disabled {
            return None;
        }
        (self.hooks.resolve_fast_paste)()
    }

    /// Waits the paste delay, then posts Command-V.
    fn paste_command(&self) -> Result<(), PasteFailure> {
        std::thread::sleep(self.hooks.paste_delay);
        if let Some(binary) = self.fast_paste_binary() {
            let output = (self.hooks.run)(&binary, &[], PASTE_TIMEOUT);
            match output.status {
                Some(0) => return Ok(()),
                // The helper found no layout key for "v": posting the US key
                // code could trigger another shortcut, so do not fall back.
                Some(3) => return Err(PasteFailure::KeyboardLayoutUnresolved),
                _ if output.timed_out => return Err(PasteFailure::TimedOut),
                status => {
                    log::warn!(
                        "[paste] fast paste failed ({status:?}: {}), falling back to osascript",
                        output.stderr.trim()
                    );
                    lock(&self.caches).fast_paste_disabled = true;
                }
            }
        }
        let output = (self.hooks.run)(Path::new(OSASCRIPT), &OSASCRIPT_PASTE, PASTE_TIMEOUT);
        if output.success() {
            return Ok(());
        }
        lock(&self.caches).accessibility = None;
        if output.timed_out {
            return Err(PasteFailure::TimedOut);
        }
        let detail = match output.status {
            Some(code) if output.stderr.trim().is_empty() => format!("code {code}"),
            Some(code) => format!("code {code}: {}", output.stderr.trim()),
            None => output.stderr.trim().to_string(),
        };
        Err(PasteFailure::CommandFailed(detail))
    }

    /// A non-silent check caches the answer for 5 s and shows the permission
    /// dialog on denial.
    fn check_accessibility(&self, silent: bool) -> bool {
        if !silent
            && let Some((value, at)) = lock(&self.caches).accessibility
            && at.elapsed() < ACCESSIBILITY_CHECK_TTL
        {
            return value;
        }
        let allowed = (self.hooks.accessibility_trusted)();
        if !silent {
            lock(&self.caches).accessibility = Some((allowed, Instant::now()));
            if !allowed {
                (self.hooks.show_accessibility_dialog)();
            }
        }
        allowed
    }
}

/// Puts `original` back after `delay`, unless the clipboard no longer holds
/// the pasted text (another copy won the race).
fn restore_after_delay(
    pasteboard: &dyn Pasteboard,
    original: &ClipboardSnapshot,
    delay: Duration,
    expected: &str,
) {
    std::thread::sleep(delay);
    if pasteboard.read_text().as_deref() != Some(expected) {
        log::debug!("[paste] skipping clipboard restore because the clipboard changed");
        return;
    }
    pasteboard.restore(original);
}

/// Pastes through the shared [`Paster`]. After `pasted`, start
/// `TextEditMonitor::shared().start_monitoring_after_paste` for auto-learn.
pub fn paste_text(text: &str, options: PasteOptions) -> PasteOutcome {
    Paster::shared().paste_text(text, options)
}

/// The Target app: the app holding keyboard focus (frontmost as fallback).
pub fn capture_frontmost_pid() -> Option<i32> {
    TextEditMonitor::shared().capture_target_pid()
}

/// Explains the missing Accessibility permission and offers to open the
/// pane. Non-blocking.
pub fn show_accessibility_dialog() {
    std::thread::spawn(|| {
        let message = "Whisper needs Accessibility permission to paste text into other applications.\\n\\n\
To fix this:\\n1. Open System Settings > Privacy & Security > Accessibility\\n\
2. Remove any old Whisper entries (they may be stale after a reinstall)\\n\
3. Add Whisper and enable the checkbox\\n4. Restart Whisper\\n\\n\
Without this permission, dictated text is only copied to the clipboard.\\n\\n\
Would you like to open System Settings now?";
        let script = format!(
            "display dialog \"{message}\" buttons {{\"Cancel\", \"Open System Settings\"}} default button \"Open System Settings\""
        );
        let opened = std::process::Command::new(OSASCRIPT)
            .args(["-e", &script])
            .status()
            .is_ok_and(|status| status.success());
        if opened {
            let _ = open_settings(SettingsPane::Accessibility);
        }
    });
}

#[cfg(test)]
mod tests {
    //! Smart spacing, clipboard restore and end-to-end paste scenarios with a
    //! fake pasteboard and fake helper runs.
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn smart_spacing() {
        assert_eq!(apply_smart_spacing("hello"), "hello ");
        assert_eq!(apply_smart_spacing("hello."), "hello. ");
        assert_eq!(apply_smart_spacing("hello!"), "hello! ");
        assert_eq!(apply_smart_spacing("hello "), "hello ");
        assert_eq!(apply_smart_spacing("hello\n"), "hello\n");
        assert_eq!(apply_smart_spacing("hello\t"), "hello\t");
        assert_eq!(apply_smart_spacing(""), "");
        for unspaced in [
            "你好",
            "日本語",
            "こんにちは",
            "カタカナ",
            "你好。",
            "すごい！",
            "何？",
            "はい、",
            "「引用」",
            "（括弧）",
            "ﾃｽﾄ｡",
            "你好︒",
            "hello 你好",
        ] {
            assert_eq!(apply_smart_spacing(unspaced), unspaced, "{unspaced}");
        }
        assert_eq!(apply_smart_spacing("你好 hello"), "你好 hello ");
        assert_eq!(apply_smart_spacing("안녕하세요"), "안녕하세요 ");
    }

    #[derive(Default)]
    struct FakePasteboard {
        current: Mutex<ClipboardSnapshot>,
        writes: Mutex<Vec<String>>,
    }

    fn text_snapshot(text: &str) -> ClipboardSnapshot {
        ClipboardSnapshot {
            items: vec![("public.utf8-plain-text".into(), text.as_bytes().to_vec())],
        }
    }

    impl Pasteboard for FakePasteboard {
        fn read_text(&self) -> Option<String> {
            lock(&self.current).text()
        }
        fn write_text(&self, text: &str) -> bool {
            *lock(&self.current) = text_snapshot(text);
            lock(&self.writes).push(format!("writeText:{text}"));
            true
        }
        fn snapshot(&self) -> ClipboardSnapshot {
            lock(&self.current).clone()
        }
        fn restore(&self, snapshot: &ClipboardSnapshot) {
            *lock(&self.current) = snapshot.clone();
            lock(&self.writes).push("restore".into());
        }
    }

    /// (program, args) of every helper run.
    type Runs = Arc<Mutex<Vec<(String, Vec<String>)>>>;

    struct Fixture {
        paster: Paster,
        pasteboard: Arc<FakePasteboard>,
        runs: Runs,
        exit_codes: Arc<Mutex<Vec<i32>>>,
        verdict: Arc<Mutex<Option<bool>>>,
        dialogs: Arc<AtomicUsize>,
    }

    fn fixture(fast_paste: bool, trusted: bool) -> Fixture {
        let pasteboard = Arc::new(FakePasteboard::default());
        let runs = Arc::new(Mutex::new(Vec::new()));
        let exit_codes = Arc::new(Mutex::new(Vec::new()));
        let verdict = Arc::new(Mutex::new(Some(true)));
        let dialogs = Arc::new(AtomicUsize::new(0));
        let (r, e, v, d) = (
            runs.clone(),
            exit_codes.clone(),
            verdict.clone(),
            dialogs.clone(),
        );
        let paster = Paster::new(PasteHooks {
            pasteboard: pasteboard.clone(),
            run: Arc::new(move |program, args, _| {
                lock(&r).push((
                    program.display().to_string(),
                    args.iter().map(|a| a.to_string()).collect(),
                ));
                let code = {
                    let mut codes = lock(&e);
                    if codes.is_empty() { 0 } else { codes.remove(0) }
                };
                RunOutput {
                    status: Some(code),
                    ..Default::default()
                }
            }),
            resolve_fast_paste: Arc::new(move || {
                fast_paste.then(|| PathBuf::from("/tmp/whisper-fast-paste"))
            }),
            accessibility_trusted: Arc::new(move || trusted),
            show_accessibility_dialog: Arc::new(move || {
                d.fetch_add(1, Ordering::SeqCst);
            }),
            activate_target: Arc::new(|pid| pid.is_some()),
            check_paste_target: Arc::new(move |_| *lock(&v)),
            paste_delay: Duration::ZERO,
            restore_delay: Duration::from_millis(20),
            retry_delay: Duration::ZERO,
        });
        Fixture {
            paster,
            pasteboard,
            runs,
            exit_codes,
            verdict,
            dialogs,
        }
    }

    fn opts(pid: Option<i32>) -> PasteOptions {
        PasteOptions {
            target_pid: pid,
            ..Default::default()
        }
    }

    fn settle(f: &Fixture) {
        // Waits for the queued restore to finish.
        f.paster.run_clipboard_operation(|| ());
    }

    #[test]
    fn restore_preserves_rich_formats() {
        let pasteboard = FakePasteboard::default();
        let rich = ClipboardSnapshot {
            items: vec![
                ("public.utf8-plain-text".into(), b"plain before".to_vec()),
                ("public.html".into(), b"<b>html before</b>".to_vec()),
                ("public.rtf".into(), b"{\\rtf1 before}".to_vec()),
                ("public.png".into(), vec![0x89, b'P', b'N', b'G']),
            ],
        };
        pasteboard.restore(&rich);
        let snapshot = pasteboard.snapshot();
        pasteboard.write_text("dictated text");
        restore_after_delay(&pasteboard, &snapshot, Duration::ZERO, "dictated text");
        assert_eq!(pasteboard.snapshot(), rich);
    }

    #[test]
    fn restore_skipped_when_another_write_wins() {
        let pasteboard = FakePasteboard::default();
        pasteboard.write_text("user copied something else");
        restore_after_delay(
            &pasteboard,
            &text_snapshot("previous clipboard"),
            Duration::ZERO,
            "dictated text",
        );
        assert_eq!(
            pasteboard.read_text().as_deref(),
            Some("user copied something else")
        );
    }

    #[test]
    fn confirmed_paste_uses_fast_paste_and_restores() {
        let f = fixture(true, true);
        f.pasteboard.write_text("previous clipboard");
        let outcome = f.paster.paste_text("final transcript", opts(Some(42)));
        assert_eq!(
            outcome,
            PasteOutcome {
                pasted: true,
                failure: None
            }
        );
        assert_eq!(lock(&f.runs)[0].0, "/tmp/whisper-fast-paste");
        settle(&f);
        assert_eq!(
            f.pasteboard.read_text().as_deref(),
            Some("previous clipboard")
        );
    }

    #[test]
    fn unconfirmed_targets_keep_the_transcript_without_pasting() {
        for fast in [true, false] {
            for verdict in [Some(false), None] {
                let f = fixture(fast, true);
                f.pasteboard.write_text("previous clipboard");
                *lock(&f.verdict) = verdict;
                let outcome = f.paster.paste_text("final transcript", opts(Some(42)));
                assert_eq!(
                    outcome,
                    PasteOutcome {
                        pasted: false,
                        failure: None
                    }
                );
                assert_eq!(
                    f.pasteboard.read_text().as_deref(),
                    Some("final transcript ")
                );
                assert!(lock(&f.runs).is_empty());
                // The queue is released for later clipboard work.
                f.paster
                    .run_clipboard_operation(|| f.pasteboard.write_text("manual copy"));
                assert_eq!(f.pasteboard.read_text().as_deref(), Some("manual copy"));
            }
        }
    }

    #[test]
    fn a_dictation_without_target_is_clipboard_only() {
        let f = fixture(true, true);
        *lock(&f.verdict) = None;
        let outcome = f.paster.paste_text("manual transcript", opts(None));
        assert!(!outcome.pasted);
        assert_eq!(
            f.pasteboard.read_text().as_deref(),
            Some("manual transcript ")
        );
    }

    #[test]
    fn unresolved_layout_leaves_text_without_fallback_or_restore() {
        let f = fixture(true, true);
        f.pasteboard.write_text("previous clipboard");
        lock(&f.exit_codes).push(3);
        let outcome = f.paster.paste_text("dictated text", opts(Some(42)));
        assert_eq!(
            outcome.failure,
            Some(PasteFailure::KeyboardLayoutUnresolved)
        );
        assert_eq!(lock(&f.runs).len(), 1, "no osascript fallback");
        settle(&f);
        assert_eq!(f.pasteboard.read_text().as_deref(), Some("dictated text "));
    }

    #[test]
    fn fast_paste_failure_falls_back_to_osascript() {
        let f = fixture(true, true);
        lock(&f.exit_codes).push(2);
        let outcome = f.paster.paste_text("dictated", opts(Some(42)));
        assert!(outcome.pasted);
        let runs = lock(&f.runs).clone();
        assert_eq!(runs[1].0, OSASCRIPT);
        assert_eq!(runs[1].1, OSASCRIPT_PASTE.map(String::from).to_vec());
        settle(&f);
        // The broken helper is not tried again.
        f.paster.paste_text("again", opts(Some(42)));
        assert_eq!(
            lock(&f.runs).last().map(|r| r.0.clone()),
            Some(OSASCRIPT.to_string())
        );
    }

    #[test]
    fn failures_retry_once() {
        let f = fixture(false, true);
        lock(&f.exit_codes).extend([1, 1]);
        let outcome = f.paster.paste_text("dictated", opts(Some(42)));
        assert_eq!(
            outcome.failure,
            Some(PasteFailure::CommandFailed("code 1".into()))
        );
        assert_eq!(lock(&f.runs).len(), 2);
    }

    #[test]
    fn missing_accessibility_fails_or_falls_back() {
        let f = fixture(true, false);
        let outcome = f.paster.paste_text("dictated", opts(Some(42)));
        assert_eq!(outcome.failure, Some(PasteFailure::AccessibilityRequired));
        assert_eq!(f.dialogs.load(Ordering::SeqCst), 1);
        let fallback = f.paster.paste_text(
            "dictated",
            PasteOptions {
                allow_clipboard_fallback: true,
                ..opts(Some(42))
            },
        );
        assert_eq!(
            fallback,
            PasteOutcome {
                pasted: false,
                failure: None
            }
        );
        assert_eq!(
            f.dialogs.load(Ordering::SeqCst),
            1,
            "silent check shows no dialog"
        );
        assert!(lock(&f.runs).is_empty());
    }

    #[test]
    fn next_paste_waits_for_the_previous_restore() {
        let f = Arc::new(fixture(true, true));
        f.pasteboard.write_text("previous clipboard");
        f.paster.paste_text("first", opts(Some(42)));
        let started = Instant::now();
        let other = Arc::clone(&f);
        let second = std::thread::spawn(move || other.paster.paste_text("second", opts(Some(42))));
        assert!(second.join().unwrap().pasted);
        assert!(started.elapsed() >= Duration::from_millis(15));
        settle(&f);
        // The first restore put the original back before the second snapshot.
        assert_eq!(
            f.pasteboard.read_text().as_deref(),
            Some("previous clipboard")
        );
    }

    #[test]
    fn no_restore_when_not_requested() {
        let f = fixture(true, true);
        f.pasteboard.write_text("previous clipboard");
        f.paster.paste_text(
            "keep me",
            PasteOptions {
                restore_clipboard: false,
                ..opts(Some(42))
            },
        );
        settle(&f);
        assert_eq!(f.pasteboard.read_text().as_deref(), Some("keep me "));
    }
}
