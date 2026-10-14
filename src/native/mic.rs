//! Microphone listing and the preferred-microphone policy.
//!
//! Devices come from CoreAudio in-process: `id` is the CoreAudio device UID,
//! which is exactly `cpal::Device::id()?.id()`, and `label` is the device
//! name (`cpal` `description().name()`), so the capture side can pick the
//! same device. Lid state comes from `macos-mic-listener --watch-lid-state`.

use std::ffi::c_void;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use unicode_normalization::UnicodeNormalization;

use crate::settings::{MicrophoneSelectionMode, Settings};

use super::helper::resolve_helper;
use super::hotkeys::globe::{
    ChildEvent, ChildSink, ListenerChild, ListenerSpawner, ProcessSpawner,
};
use super::timer::{RealTimers, TimerId, Timers, lock};

/// An audio input device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicDevice {
    /// CoreAudio device UID (stable across reboots for most devices).
    pub id: String,
    /// CoreAudio device name, as shown in System Settings and by `cpal`.
    pub label: String,
    /// CoreAudio reports a built-in transport.
    pub is_built_in: bool,
    /// The current system default input.
    pub is_default: bool,
}

// ---------------------------------------------------------------------------
// CoreAudio

mod coreaudio {
    use super::*;

    pub type AudioObjectId = u32;

    #[repr(C)]
    pub struct PropertyAddress {
        pub selector: u32,
        pub scope: u32,
        pub element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: AudioObjectId,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            object: AudioObjectId,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    const fn fourcc(code: &[u8; 4]) -> u32 {
        ((code[0] as u32) << 24)
            | ((code[1] as u32) << 16)
            | ((code[2] as u32) << 8)
            | code[3] as u32
    }

    const SYSTEM_OBJECT: AudioObjectId = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const SCOPE_INPUT: u32 = fourcc(b"inpt");
    const ELEMENT_MAIN: u32 = 0;
    const DEVICES: u32 = fourcc(b"dev#");
    const DEFAULT_INPUT: u32 = fourcc(b"dIn ");
    const STREAM_CONFIGURATION: u32 = fourcc(b"slay");
    const NAME: u32 = fourcc(b"lnam");
    const UID: u32 = fourcc(b"uid ");
    const TRANSPORT_TYPE: u32 = fourcc(b"tran");
    const TRANSPORT_BUILT_IN: u32 = fourcc(b"bltn");

    fn address(selector: u32, scope: u32) -> PropertyAddress {
        PropertyAddress {
            selector,
            scope,
            element: ELEMENT_MAIN,
        }
    }

    fn data_size(object: AudioObjectId, address: &PropertyAddress) -> Option<u32> {
        let mut size = 0u32;
        // SAFETY: valid address and out-pointer.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(object, address, 0, std::ptr::null(), &mut size)
        };
        (status == 0).then_some(size)
    }

    fn read_u32(object: AudioObjectId, selector: u32) -> Option<u32> {
        let address = address(selector, SCOPE_GLOBAL);
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: the property is a UInt32 and `value` has room for it.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                (&mut value as *mut u32).cast(),
            )
        };
        (status == 0).then_some(value)
    }

    fn read_string(object: AudioObjectId, selector: u32) -> Option<String> {
        let address = address(selector, SCOPE_GLOBAL);
        let mut value: CFStringRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        // SAFETY: the property is a CFStringRef returned under the create rule.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                (&mut value as *mut CFStringRef).cast(),
            )
        };
        if status != 0 || value.is_null() {
            return None;
        }
        // SAFETY: non-null CFString we own.
        Some(unsafe { CFString::wrap_under_create_rule(value) }.to_string())
    }

    fn input_channels(device: AudioObjectId) -> u32 {
        let address = address(STREAM_CONFIGURATION, SCOPE_INPUT);
        let Some(size) = data_size(device, &address).filter(|s| *s >= 8) else {
            return 0;
        };
        // AudioBufferList: UInt32 count, then {UInt32 channels, UInt32 bytes,
        // void *data} per buffer (16 bytes, 8-aligned on 64-bit).
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let mut size = size;
        // SAFETY: `buffer` holds `size` bytes, 8-aligned.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                buffer.as_mut_ptr().cast(),
            )
        };
        if status != 0 {
            return 0;
        }
        let bytes: &[u8] =
            // SAFETY: reinterpreting the initialized u64 buffer as bytes.
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), size as usize) };
        let read = |offset: usize| -> u32 {
            bytes
                .get(offset..offset + 4)
                .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                .unwrap_or(0)
        };
        let count = read(0) as usize;
        (0..count).map(|i| read(8 + i * 16)).sum()
    }

    pub fn default_input() -> Option<AudioObjectId> {
        read_u32(SYSTEM_OBJECT, DEFAULT_INPUT).filter(|id| *id != 0)
    }

    pub fn device_name(device: AudioObjectId) -> Option<String> {
        read_string(device, NAME).filter(|n| !n.trim().is_empty())
    }

    pub fn device_uid(device: AudioObjectId) -> Option<String> {
        read_string(device, UID)
    }

    pub fn input_devices() -> Vec<MicDevice> {
        let address = address(DEVICES, SCOPE_GLOBAL);
        let Some(size) = data_size(SYSTEM_OBJECT, &address) else {
            return Vec::new();
        };
        let count = size as usize / std::mem::size_of::<AudioObjectId>();
        let mut ids = vec![0 as AudioObjectId; count];
        let mut size = size;
        // SAFETY: `ids` has room for `size` bytes of device ids.
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                ids.as_mut_ptr().cast(),
            )
        };
        if status != 0 {
            return Vec::new();
        }
        ids.truncate(size as usize / std::mem::size_of::<AudioObjectId>());
        let default = default_input();
        ids.into_iter()
            .filter(|id| input_channels(*id) > 0)
            .filter_map(|id| {
                Some(MicDevice {
                    id: device_uid(id)?,
                    label: device_name(id).unwrap_or_default(),
                    is_built_in: read_u32(id, TRANSPORT_TYPE) == Some(TRANSPORT_BUILT_IN),
                    is_default: default == Some(id),
                })
            })
            .collect()
    }
}

/// Every audio input device, with the system default flagged.
pub fn list_input_devices() -> Vec<MicDevice> {
    coreaudio::input_devices()
}

/// The system default input (name and CoreAudio UID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDefaultMicrophone {
    pub name: String,
    pub native_id: Option<String>,
}

/// Reads the system default input in-process.
pub fn system_default_microphone() -> Option<SystemDefaultMicrophone> {
    let id = coreaudio::default_input()?;
    Some(SystemDefaultMicrophone {
        name: coreaudio::device_name(id)?.trim().to_string(),
        native_id: coreaudio::device_uid(id).filter(|uid| !uid.is_empty()),
    })
}

// ---------------------------------------------------------------------------
// Selection policy

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicSelectionMode {
    Auto,
    System,
    BuiltIn,
    Specific,
}

impl MicSelectionMode {
    pub fn from_settings(mode: MicrophoneSelectionMode) -> Self {
        match mode {
            MicrophoneSelectionMode::Auto => Self::Auto,
            MicrophoneSelectionMode::BuiltIn => Self::BuiltIn,
            MicrophoneSelectionMode::Specific => Self::Specific,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicStatus {
    AutoBuiltIn,
    AutoExternal,
    NativeExact,
    NativeCompatible,
    NativeUnmatched,
    Unavailable,
    BuiltIn,
    Exact,
    Remapped,
    Missing,
    Ambiguous,
    Default,
}

impl MicStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoBuiltIn => "auto-built-in",
            Self::AutoExternal => "auto-external",
            Self::NativeExact => "native-exact",
            Self::NativeCompatible => "native-compatible",
            Self::NativeUnmatched => "native-unmatched",
            Self::Unavailable => "unavailable",
            Self::BuiltIn => "built-in",
            Self::Exact => "exact",
            Self::Remapped => "remapped",
            Self::Missing => "missing",
            Self::Ambiguous => "ambiguous",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicResolution {
    pub mode: MicSelectionMode,
    /// `None` means "use the system default input".
    pub device: Option<MicDevice>,
    pub status: MicStatus,
    pub lid_closed: Option<bool>,
}

impl MicResolution {
    /// A resolved physical device can be cached by the capture side.
    pub fn is_cacheable(&self) -> bool {
        self.device.as_ref().is_some_and(|d| !d.id.is_empty())
    }

    /// (id, label) the caller should save as the selected device: a stale id
    /// was remapped by label, or no label had been saved yet.
    pub fn selection_to_persist(&self, saved_label: &str) -> Option<(String, String)> {
        let device = self.device.as_ref()?;
        (self.mode == MicSelectionMode::Specific
            && (self.status == MicStatus::Remapped || saved_label.is_empty()))
        .then(|| (device.id.clone(), device.label.clone()))
    }
}

/// Lowercased, NFKC, without "Default - " / "(Built-in)" decorations.
pub fn normalize_microphone_label(label: &str) -> String {
    let mut text: String = label.nfkc().collect();
    let trimmed = text.trim_start();
    if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("default") {
        let rest = trimmed[7..].trim_start();
        if let Some(rest) = rest.strip_prefix('-') {
            text = rest.trim_start().to_string();
        }
    }
    let trimmed_end = text.trim_end();
    if let Some(open) = trimmed_end.rfind('(')
        && trimmed_end.ends_with(')')
    {
        let inner = trimmed_end[open + 1..trimmed_end.len() - 1].to_lowercase();
        let decoration = matches!(
            inner.as_str(),
            "builtin" | "built-in" | "built in" | "default"
        );
        if decoration {
            text = trimmed_end[..open].to_string();
        }
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn comparable_label(label: &str) -> String {
    normalize_microphone_label(label)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub fn is_built_in_microphone(label: &str) -> bool {
    let lower = label.to_lowercase();
    if ["built-in", "internal", "macbook", "integrated"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return true;
    }
    if lower.contains("microphone") {
        let without = lower.replace("microphone", " ");
        const EXTERNAL: [&str; 12] = [
            "bluetooth",
            "airpods",
            "wireless",
            "usb",
            "external",
            "headset",
            "webcam",
            "iphone",
            "ipad",
            "cell",
            "phone",
            "continuity",
        ];
        return !EXTERNAL.iter().any(|k| without.contains(k));
    }
    false
}

fn auto_built_in_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    lower.contains("built-in")
        || lower.contains("built in")
        || lower.contains("builtin")
        || lower.contains("internal")
        || lower.contains("macbook")
        || lower.contains("integrated")
}

pub fn resolve_system_default_device(
    devices: &[MicDevice],
    system_default_name: Option<&str>,
) -> (Option<MicDevice>, MicStatus) {
    let native = system_default_name.map(str::trim).filter(|n| !n.is_empty());

    if let Some(name) = native {
        let target = comparable_label(name);
        let exact: Vec<_> = devices
            .iter()
            .filter(|d| !target.is_empty() && comparable_label(&d.label) == target)
            .collect();
        if exact.len() == 1 {
            return (Some(exact[0].clone()), MicStatus::NativeExact);
        }
        let contained: Vec<_> = devices
            .iter()
            .filter(|d| {
                let candidate = comparable_label(&d.label);
                target.chars().count() >= 8
                    && candidate.chars().count() >= 8
                    && (candidate.contains(&target) || target.contains(&candidate))
            })
            .collect();
        if contained.len() == 1 {
            return (Some(contained[0].clone()), MicStatus::NativeCompatible);
        }
    }

    // CoreAudio flags the default directly.
    if let Some(device) = devices.iter().find(|d| d.is_default) {
        return (Some(device.clone()), MicStatus::NativeExact);
    }
    let status = if native.is_some() {
        MicStatus::NativeUnmatched
    } else {
        MicStatus::Unavailable
    };
    (None, status)
}

/// The saved id while it exists, else a unique label match.
pub fn resolve_mic_device_selection(
    devices: &[MicDevice],
    selected_id: &str,
    selected_label: &str,
) -> (Option<MicDevice>, MicStatus) {
    if selected_id.is_empty() {
        return (None, MicStatus::Default);
    }
    if let Some(device) = devices.iter().find(|d| d.id == selected_id) {
        return (Some(device.clone()), MicStatus::Exact);
    }
    if selected_label.is_empty() {
        return (None, MicStatus::Missing);
    }
    let matches: Vec<_> = devices
        .iter()
        .filter(|d| d.label == selected_label)
        .collect();
    match matches.len() {
        1 => (Some(matches[0].clone()), MicStatus::Remapped),
        0 => (None, MicStatus::Missing),
        _ => (None, MicStatus::Ambiguous),
    }
}

/// Saved preference inputs for [`resolve_microphone_selection`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicPreference {
    pub mode: MicSelectionMode,
    pub selected_id: String,
    pub selected_label: String,
}

impl MicPreference {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            mode: MicSelectionMode::from_settings(settings.microphone_selection_mode),
            selected_id: settings.selected_mic_device_id.clone(),
            selected_label: settings.selected_mic_device_label.clone(),
        }
    }
}

/// Auto prefers the built-in input, unless the lid is closed, where it prefers
/// a phone (Continuity), then any other external input; with neither it
/// follows the system default.
pub fn resolve_microphone_selection(
    devices: &[MicDevice],
    preference: &MicPreference,
    system_default_name: Option<&str>,
    lid_closed: Option<bool>,
) -> MicResolution {
    let mode = preference.mode;
    let resolution = |(device, status): (Option<MicDevice>, MicStatus)| MicResolution {
        mode,
        device,
        status,
        lid_closed: None,
    };
    match mode {
        MicSelectionMode::Auto => {
            let physical: Vec<&MicDevice> = devices.iter().filter(|d| !d.id.is_empty()).collect();
            // macOS names its internal input explicitly; the generic
            // "Microphone" heuristic also matches custom-named external mics.
            let built_in: Vec<&MicDevice> = physical
                .iter()
                .copied()
                .filter(|d| auto_built_in_label(&d.label) || d.is_built_in)
                .collect();
            let external: Vec<&MicDevice> = physical
                .iter()
                .copied()
                .filter(|d| !d.label.is_empty() && !built_in.iter().any(|b| b.id == d.id))
                .collect();
            let phone = external.iter().find(|d| {
                let lower = d.label.to_lowercase();
                lower.contains("iphone") || lower.contains("continuity")
            });
            let device = if lid_closed == Some(true) {
                phone.or(external.first()).copied()
            } else {
                built_in.first().copied()
            };
            if let Some(device) = device {
                return MicResolution {
                    mode,
                    device: Some(device.clone()),
                    status: if lid_closed == Some(true) {
                        MicStatus::AutoExternal
                    } else {
                        MicStatus::AutoBuiltIn
                    },
                    lid_closed,
                };
            }
            MicResolution {
                lid_closed,
                ..resolution(resolve_system_default_device(devices, system_default_name))
            }
        }
        MicSelectionMode::System => {
            resolution(resolve_system_default_device(devices, system_default_name))
        }
        MicSelectionMode::BuiltIn => {
            let device = devices
                .iter()
                .find(|d| is_built_in_microphone(&d.label))
                .cloned();
            let status = if device.is_some() {
                MicStatus::BuiltIn
            } else {
                MicStatus::Unavailable
            };
            resolution((device, status))
        }
        MicSelectionMode::Specific => resolution(resolve_mic_device_selection(
            devices,
            &preference.selected_id,
            &preference.selected_label,
        )),
    }
}

/// Lists devices, reads lid state (Auto) and the system default, and applies
/// the policy. Blocks up to ~1.5 s on the first lid read; call off the main
/// thread. With `force_system_default`, ignores the preference.
pub fn resolve_preferred_microphone_with(
    settings: &Settings,
    force_system_default: bool,
) -> MicResolution {
    let mut preference = MicPreference::from_settings(settings);
    if force_system_default {
        preference.mode = MicSelectionMode::System;
    }
    let devices = list_input_devices();
    let lid_closed = (preference.mode == MicSelectionMode::Auto)
        .then(|| LaptopLidMonitor::shared().state())
        .flatten();
    let system_default = matches!(
        preference.mode,
        MicSelectionMode::Auto | MicSelectionMode::System
    )
    .then(system_default_microphone)
    .flatten();
    resolve_microphone_selection(
        &devices,
        &preference,
        system_default.as_ref().map(|d| d.name.as_str()),
        lid_closed,
    )
}

/// The preferred microphone for `settings` (see
/// [`resolve_preferred_microphone_with`]).
pub fn resolve_preferred_microphone(settings: &Settings) -> MicResolution {
    resolve_preferred_microphone_with(settings, false)
}

// ---------------------------------------------------------------------------
// Laptop lid

const LID_INITIAL_WAIT_MS: u64 = 1500;
const LID_STALE_STATE_MS: u64 = 5000;
const LID_RESTART_DELAY_MS: u64 = 2000;
const LID_MAX_BUFFER: usize = 4096;
const MIC_LISTENER_BINARY: &str = "macos-mic-listener";

fn lid_subscribers() -> &'static Mutex<Vec<async_channel::Sender<Option<bool>>>> {
    static SUBSCRIBERS: OnceLock<Mutex<Vec<async_channel::Sender<Option<bool>>>>> = OnceLock::new();
    SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// `{"lidClosed":true|false|null}` -> Some(state); anything else -> None.
pub fn parse_lid_state(line: &str) -> Option<Option<bool>> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    match value.get("lidClosed")? {
        serde_json::Value::Null => Some(None),
        serde_json::Value::Bool(closed) => Some(Some(*closed)),
        _ => None,
    }
}

pub struct LidHooks {
    pub spawner: Arc<dyn ListenerSpawner>,
    pub resolve_binary: Arc<dyn Fn() -> Option<std::path::PathBuf> + Send + Sync>,
    pub timers: Arc<dyn Timers>,
    /// Called with every change of the known lid state.
    pub on_change: Arc<dyn Fn(Option<bool>) + Send + Sync>,
}

struct Waiter {
    id: u64,
    tx: mpsc::Sender<Option<bool>>,
    timer: Option<TimerId>,
}

#[derive(Default)]
struct LidState {
    running: bool,
    child: Option<(u64, Box<dyn ListenerChild>)>,
    generation: u64,
    restart_timer: Option<TimerId>,
    stale_timer: Option<TimerId>,
    state: Option<bool>,
    received: bool,
    last_sample_at: u64,
    buffer: String,
    waiters: Vec<Waiter>,
    next_waiter: u64,
}

struct LidInner {
    state: Mutex<LidState>,
    hooks: LidHooks,
    this: Weak<LidInner>,
}

/// Watches the clamshell state through the mic listener helper, which emits
/// a heartbeat every second; a silent helper is replaced.
#[derive(Clone)]
pub struct LaptopLidMonitor {
    inner: Arc<LidInner>,
}

impl LaptopLidMonitor {
    pub fn new(hooks: LidHooks) -> Self {
        Self {
            inner: Arc::new_cyclic(|this| LidInner {
                state: Mutex::new(LidState::default()),
                hooks,
                this: this.clone(),
            }),
        }
    }

    /// The process-wide monitor used by [`resolve_preferred_microphone`].
    pub fn shared() -> LaptopLidMonitor {
        static SHARED: OnceLock<LaptopLidMonitor> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                LaptopLidMonitor::new(LidHooks {
                    spawner: Arc::new(ProcessSpawner),
                    resolve_binary: Arc::new(|| resolve_helper(MIC_LISTENER_BINARY)),
                    timers: RealTimers::shared(),
                    on_change: Arc::new(|state| {
                        lock(lid_subscribers()).retain(|tx| tx.try_send(state).is_ok());
                    }),
                })
            })
            .clone()
    }

    /// Lid changes of the shared monitor (`Some(true)` closed, `None`
    /// unknown).
    pub fn subscribe_shared() -> async_channel::Receiver<Option<bool>> {
        let (tx, rx) = async_channel::unbounded();
        lock(lid_subscribers()).push(tx);
        Self::shared().start();
        rx
    }

    pub fn start(&self) {
        let mut state = lock(&self.inner.state);
        if state.running {
            return;
        }
        state.running = true;
        self.inner.launch(&mut state);
    }

    /// The current lid state: answered at once from a fresh sample, otherwise
    /// after the next sample or a 1.5 s bound (then `None`).
    pub fn request_state(&self) -> mpsc::Receiver<Option<bool>> {
        let (tx, rx) = mpsc::channel();
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        if !state.running {
            state.running = true;
            inner.launch(&mut state);
        }
        let now = inner.hooks.timers.now_ms();
        if state.received && now.saturating_sub(state.last_sample_at) < LID_STALE_STATE_MS {
            let _ = tx.send(state.state);
            return rx;
        }
        // Timers can be delayed by sleep; never answer with a pre-sleep value.
        if state.received {
            let generation = state.child.as_ref().map(|(g, _)| *g);
            if let Some(generation) = generation {
                inner.retire(
                    &mut state,
                    generation,
                    Some("Cached lid state expired"),
                    true,
                );
            }
        }
        state.next_waiter += 1;
        let id = state.next_waiter;
        let weak = inner.this.clone();
        let timer = inner.hooks.timers.schedule(
            Duration::from_millis(LID_INITIAL_WAIT_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    if let Some(index) = state.waiters.iter().position(|w| w.id == id) {
                        let waiter = state.waiters.remove(index);
                        let _ = waiter.tx.send(None);
                    }
                }
            }),
        );
        state.waiters.push(Waiter {
            id,
            tx,
            timer: Some(timer),
        });
        rx
    }

    /// Blocking form of [`LaptopLidMonitor::request_state`].
    pub fn state(&self) -> Option<bool> {
        self.request_state()
            .recv_timeout(Duration::from_millis(LID_INITIAL_WAIT_MS + 500))
            .ok()
            .flatten()
    }

    pub fn stop(&self) {
        let inner = &self.inner;
        let mut state = lock(&inner.state);
        state.running = false;
        if let Some(timer) = state.restart_timer.take() {
            inner.hooks.timers.cancel(timer);
        }
        let previous = state.child.take();
        inner.invalidate(&mut state);
        inner.finish_waiters(&mut state, None);
        drop(state);
        if let Some((_, mut child)) = previous {
            child.kill(libc::SIGTERM);
        }
    }
}

impl LidInner {
    fn update_state(&self, state: &mut LidState, value: Option<bool>) {
        if state.state != value {
            state.state = value;
            (self.hooks.on_change)(value);
        }
    }

    fn invalidate(&self, state: &mut LidState) {
        state.received = false;
        state.last_sample_at = 0;
        if let Some(timer) = state.stale_timer.take() {
            self.hooks.timers.cancel(timer);
        }
        self.update_state(state, None);
    }

    fn finish_waiters(&self, state: &mut LidState, value: Option<bool>) {
        for waiter in state.waiters.drain(..) {
            if let Some(timer) = waiter.timer {
                self.hooks.timers.cancel(timer);
            }
            let _ = waiter.tx.send(value);
        }
    }

    fn schedule_restart(&self, state: &mut LidState) {
        if !state.running || state.restart_timer.is_some() {
            return;
        }
        let weak = self.this.clone();
        state.restart_timer = Some(self.hooks.timers.schedule(
            Duration::from_millis(LID_RESTART_DELAY_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    state.restart_timer = None;
                    inner.launch(&mut state);
                }
            }),
        ));
    }

    fn retire(&self, state: &mut LidState, generation: u64, error: Option<&str>, kill: bool) {
        if !state.child.as_ref().is_some_and(|(g, _)| *g == generation) {
            return;
        }
        let child = state.child.take();
        self.invalidate(state);
        self.finish_waiters(state, None);
        if let Some(error) = error {
            log::warn!("[lid] laptop lid monitor unavailable: {error}");
        }
        if let (true, Some((_, mut child))) = (kill, child) {
            child.kill(libc::SIGTERM);
        }
        self.schedule_restart(state);
    }

    fn arm_stale_timer(&self, state: &mut LidState, generation: u64) {
        if let Some(timer) = state.stale_timer.take() {
            self.hooks.timers.cancel(timer);
        }
        let weak = self.this.clone();
        state.stale_timer = Some(self.hooks.timers.schedule(
            Duration::from_millis(LID_STALE_STATE_MS),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock(&inner.state);
                    state.stale_timer = None;
                    inner.retire(
                        &mut state,
                        generation,
                        Some("Lid state heartbeat timed out"),
                        true,
                    );
                }
            }),
        ));
    }

    fn launch(&self, state: &mut LidState) {
        if !state.running || state.child.is_some() {
            return;
        }
        let Some(binary) = (self.hooks.resolve_binary)() else {
            self.schedule_restart(state);
            return;
        };
        state.generation += 1;
        let generation = state.generation;
        let weak = self.this.clone();
        let sink: ChildSink = Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_child_event(generation, event);
            }
        });
        match self
            .hooks
            .spawner
            .spawn(&binary, &["--watch-lid-state".to_string()], sink)
        {
            Ok(child) => {
                state.child = Some((generation, child));
                state.buffer.clear();
                self.arm_stale_timer(state, generation);
            }
            Err(error) => {
                log::warn!("[lid] cannot start the lid monitor: {error}");
                self.schedule_restart(state);
            }
        }
    }

    fn on_child_event(&self, generation: u64, event: ChildEvent) {
        let mut state = lock(&self.state);
        let current = state.child.as_ref().is_some_and(|(g, _)| *g == generation);
        if !current || !state.running {
            return;
        }
        match event {
            ChildEvent::Stdout(chunk) => {
                state.buffer.push_str(&chunk);
                if state.buffer.len() > LID_MAX_BUFFER {
                    self.retire(
                        &mut state,
                        generation,
                        Some("Invalid lid state output"),
                        true,
                    );
                    return;
                }
                while let Some(newline) = state.buffer.find('\n') {
                    let line: String = state.buffer.drain(..=newline).collect();
                    let Some(value) = parse_lid_state(line.trim()) else {
                        continue;
                    };
                    state.last_sample_at = self.hooks.timers.now_ms();
                    state.received = true;
                    self.update_state(&mut state, value);
                    self.finish_waiters(&mut state, value);
                    self.arm_stale_timer(&mut state, generation);
                }
            }
            ChildEvent::Stderr(_) => {}
            ChildEvent::Error(error) => self.retire(&mut state, generation, Some(&error), true),
            ChildEvent::Exit { .. } => self.retire(&mut state, generation, None, false),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Microphone selection, device selection and the laptop lid monitor.
    use super::*;
    use crate::native::hotkeys::globe::tests::FakeSpawner;
    use crate::native::timer::FakeTimers;
    use std::path::PathBuf;

    fn mic(id: &str, label: &str) -> MicDevice {
        MicDevice {
            id: id.into(),
            label: label.into(),
            is_built_in: false,
            is_default: false,
        }
    }

    fn pref(mode: MicSelectionMode) -> MicPreference {
        MicPreference {
            mode,
            selected_id: String::new(),
            selected_label: String::new(),
        }
    }

    #[test]
    fn auto_follows_lid_changes() {
        let built_in = mic("macbook", "MacBook Pro Microphone (Built-in)");
        let phone = mic("iphone", "Rui's iPhone 16 Pro Max Microphone");
        let usb = mic("usb", "USB Microphone");
        let devices = vec![usb, phone.clone(), built_in.clone()];
        for lid in [false, true, false, true] {
            let result = resolve_microphone_selection(
                &devices,
                &pref(MicSelectionMode::Auto),
                Some(&phone.label),
                Some(lid),
            );
            assert_eq!(
                result.device.as_ref(),
                Some(if lid { &phone } else { &built_in })
            );
        }
    }

    #[test]
    fn auto_with_lid_closed_tracks_external_devices() {
        let built_in = mic("macbook", "MacBook Pro Microphone");
        let phone = mic("iphone", "Rui's iPhone Microphone");
        let external = mic("usb", "Studio Microphone");
        let resolve = |devices: Vec<MicDevice>| {
            resolve_microphone_selection(&devices, &pref(MicSelectionMode::Auto), None, Some(true))
                .device
        };
        assert_eq!(
            resolve(vec![built_in.clone(), external.clone()]),
            Some(external.clone())
        );
        assert_eq!(
            resolve(vec![built_in.clone(), external.clone(), phone.clone()]),
            Some(phone)
        );
        assert_eq!(
            resolve(vec![built_in.clone(), external.clone()]),
            Some(external)
        );
        assert_eq!(resolve(vec![built_in]), None);
    }

    #[test]
    fn auto_with_lid_closed_falls_back_to_system_default() {
        let mut built_in = mic("macbook", "MacBook Pro Microphone");
        built_in.is_default = true;
        let result = resolve_microphone_selection(
            std::slice::from_ref(&built_in),
            &pref(MicSelectionMode::Auto),
            None,
            Some(true),
        );
        assert_eq!(result.device, Some(built_in));
        assert_eq!(result.status, MicStatus::NativeExact);
    }

    #[test]
    fn auto_with_unknown_lid_prefers_built_in() {
        let built_in = mic("macbook", "MacBook Pro Microphone");
        let result = resolve_microphone_selection(
            &[mic("continuity", "Joshua P Microphone"), built_in.clone()],
            &pref(MicSelectionMode::Auto),
            None,
            None,
        );
        assert_eq!(result.device, Some(built_in));
    }

    #[test]
    fn manual_input_ignores_the_lid() {
        let built_in = mic("macbook", "MacBook Pro Microphone");
        let phone = mic("iphone", "Rui's iPhone Microphone");
        for lid in [true, false] {
            let result = resolve_microphone_selection(
                &[built_in.clone(), phone.clone()],
                &MicPreference {
                    mode: MicSelectionMode::Specific,
                    selected_id: "iphone".into(),
                    selected_label: String::new(),
                },
                None,
                Some(lid),
            );
            assert_eq!(result.device, Some(phone.clone()));
        }
    }

    #[test]
    fn system_mode_mapping() {
        let expected = mic("macbook", "MacBook Pro Microphone (Built-in)");
        let result = resolve_microphone_selection(
            &[mic("usb", "USB Microphone"), expected.clone()],
            &pref(MicSelectionMode::System),
            Some("MacBook Pro Microphone"),
            None,
        );
        assert_eq!(result.device, Some(expected));
        assert_eq!(result.status, MicStatus::NativeExact);

        let result = resolve_microphone_selection(
            &[
                mic("continuity", "Joshua P Microphone"),
                mic("usb", "USB Microphone"),
            ],
            &pref(MicSelectionMode::System),
            Some("USB Microphone"),
            None,
        );
        assert!(result.is_cacheable());

        let result = resolve_microphone_selection(
            &[
                mic("continuity", "Joshua P Microphone"),
                mic("usb", "USB Microphone"),
            ],
            &pref(MicSelectionMode::System),
            Some("Missing System Microphone"),
            None,
        );
        assert_eq!(result.device, None);
        assert_eq!(result.status, MicStatus::NativeUnmatched);

        let expected = mic("7a1b", "Jabra Evolve2 65 Microphone");
        let result = resolve_microphone_selection(
            &[
                mic("macbook", "MacBook Pro Microphone (Built-in)"),
                expected.clone(),
            ],
            &pref(MicSelectionMode::System),
            Some("Jabra Evolve2 65"),
            None,
        );
        assert_eq!(result.device, Some(expected));
        assert_eq!(result.status, MicStatus::NativeCompatible);
    }

    #[test]
    fn coreaudio_default_flag_is_used() {
        let mut default = mic("uid-1", "Desk Microphone");
        default.is_default = true;
        let result = resolve_microphone_selection(
            &[mic("uid-2", "Other"), default.clone()],
            &pref(MicSelectionMode::System),
            None,
            None,
        );
        assert_eq!(result.device, Some(default));
    }

    #[test]
    fn built_in_mode_pins_the_physical_device() {
        let expected = mic("macbook", "MacBook Pro Microphone (Built-in)");
        let result = resolve_microphone_selection(
            &[expected.clone(), mic("7a1b", "Jabra Evolve2 65")],
            &pref(MicSelectionMode::BuiltIn),
            None,
            None,
        );
        assert_eq!(result.device, Some(expected));
        assert_eq!(result.status, MicStatus::BuiltIn);
    }

    #[test]
    fn device_selection_by_id_then_unique_label() {
        let device = mic("saved-id", "Studio Mic");
        assert_eq!(
            resolve_mic_device_selection(std::slice::from_ref(&device), "saved-id", "Studio Mic"),
            (Some(device), MicStatus::Exact)
        );
        let device = mic("new-id", "Studio Mic");
        assert_eq!(
            resolve_mic_device_selection(std::slice::from_ref(&device), "old-id", "Studio Mic"),
            (Some(device.clone()), MicStatus::Remapped)
        );
        assert_eq!(
            resolve_mic_device_selection(
                &[mic("first", "USB Audio"), mic("second", "USB Audio")],
                "old-id",
                "USB Audio"
            ),
            (None, MicStatus::Ambiguous)
        );
        assert_eq!(
            resolve_mic_device_selection(&[], "old-id", "Studio Mic"),
            (None, MicStatus::Missing)
        );
        assert_eq!(
            resolve_mic_device_selection(std::slice::from_ref(&device), "old-id", ""),
            (None, MicStatus::Missing)
        );
        assert_eq!(
            resolve_mic_device_selection(&[device], "", ""),
            (None, MicStatus::Default)
        );
    }

    #[test]
    fn labels_normalize() {
        assert_eq!(
            normalize_microphone_label("Default - MacBook Pro Microphone (Built-in)"),
            "macbook pro microphone"
        );
        assert_eq!(
            normalize_microphone_label("  USB   Mic (default) "),
            "usb mic"
        );
        assert!(is_built_in_microphone("MacBook Air Microphone"));
        assert!(!is_built_in_microphone("Galaxy Cell Microphone"));
        assert!(!is_built_in_microphone("AirPods Pro"));
    }

    #[test]
    fn remapped_selection_is_persisted() {
        let result = resolve_microphone_selection(
            &[mic("new-id", "Studio Mic")],
            &MicPreference {
                mode: MicSelectionMode::Specific,
                selected_id: "old-id".into(),
                selected_label: "Studio Mic".into(),
            },
            None,
            None,
        );
        assert_eq!(
            result.selection_to_persist("Studio Mic"),
            Some(("new-id".into(), "Studio Mic".into()))
        );
    }

    #[test]
    #[ignore = "reads real audio hardware"]
    fn lists_real_devices() {
        let devices = list_input_devices();
        assert!(devices.iter().all(|d| !d.id.is_empty()));
        println!("{devices:#?} default={:?}", system_default_microphone());
    }

    // Laptop lid monitor.

    struct Lid {
        monitor: LaptopLidMonitor,
        spawner: Arc<FakeSpawner>,
        timers: Arc<FakeTimers>,
        broadcasts: Arc<Mutex<Vec<Option<bool>>>>,
    }

    fn lid(binary: Option<&'static str>) -> (Lid, Arc<std::sync::atomic::AtomicUsize>) {
        let spawner = Arc::new(FakeSpawner::default());
        let timers = FakeTimers::starting_at(100);
        let broadcasts = Arc::new(Mutex::new(Vec::new()));
        let lookups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (b, l) = (broadcasts.clone(), lookups.clone());
        let monitor = LaptopLidMonitor::new(LidHooks {
            spawner: spawner.clone(),
            resolve_binary: Arc::new(move || {
                l.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                binary.map(PathBuf::from)
            }),
            timers: timers.clone(),
            on_change: Arc::new(move |v| lock(&b).push(v)),
        });
        (
            Lid {
                monitor,
                spawner,
                timers,
                broadcasts,
            },
            lookups,
        )
    }

    fn helper() -> (Lid, Arc<std::sync::atomic::AtomicUsize>) {
        lid(Some("/helper/macos-mic-listener"))
    }

    impl Lid {
        fn out(&self, index: usize, text: &str) {
            self.spawner.emit(index, ChildEvent::Stdout(text.into()));
        }
        fn broadcasts(&self) -> Vec<Option<bool>> {
            lock(&self.broadcasts).clone()
        }
        fn kills(&self, index: usize) -> usize {
            lock(&self.spawner.child(index)).kills.len()
        }
    }

    #[test]
    fn parses_only_explicit_states() {
        assert_eq!(parse_lid_state("{\"lidClosed\":true}"), Some(Some(true)));
        assert_eq!(parse_lid_state("{\"lidClosed\":false}"), Some(Some(false)));
        assert_eq!(parse_lid_state("{\"lidClosed\":null}"), Some(None));
        for value in [
            "{\"lidClosed\":0}",
            "{\"lidClosed\":\"true\"}",
            "{}",
            "null",
            "bad",
        ] {
            assert_eq!(parse_lid_state(value), None, "{value}");
        }
    }

    #[test]
    fn concurrent_lookups_share_a_helper_and_parse_partial_lines() {
        let (h, _) = helper();
        let first = h.monitor.request_state();
        let second = h.monitor.request_state();
        assert_eq!(h.spawner.count(), 1);
        assert_eq!(h.spawner.args(0), vec!["--watch-lid-state".to_string()]);
        h.out(0, "{\"lidClo");
        h.out(0, "sed\":false}\n{\"lidClosed\":false}\n");
        assert_eq!(first.try_recv().unwrap(), Some(false));
        assert_eq!(second.try_recv().unwrap(), Some(false));
        assert_eq!(h.broadcasts(), vec![Some(false)]);
        h.out(0, "{\"lidClosed\":true}\n{\"lidClosed\":null}\n");
        assert_eq!(h.broadcasts(), vec![Some(false), Some(true), None]);
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), None);
        h.monitor.stop();
    }

    #[test]
    fn initial_lookup_is_bounded() {
        let (h, _) = helper();
        let result = h.monitor.request_state();
        h.out(0, "CAPABILITY PID\n{\"lidClosed\":1}\n");
        h.timers.advance(1500);
        assert_eq!(result.try_recv().unwrap(), None);
        h.monitor.stop();
    }

    #[test]
    fn helper_failure_clears_state_and_restarts_once() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":true}\n");
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), Some(true));
        h.spawner
            .emit(0, ChildEvent::Error("process failed".into()));
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(1),
                signal: None,
            },
        );
        assert_eq!(h.broadcasts(), vec![Some(true), None]);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.out(0, "{\"lidClosed\":true}\n");
        h.out(1, "{\"lidClosed\":false}\n");
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), Some(false));
        assert_eq!(h.broadcasts(), vec![Some(true), None, Some(false)]);
        h.monitor.stop();
    }

    #[test]
    fn heartbeats_refresh_without_duplicate_broadcasts() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":false}\n");
        h.timers.advance(4000);
        h.out(0, "{\"lidClosed\":false}\n");
        h.timers.advance(4000);
        assert_eq!(h.monitor.request_state().try_recv().unwrap(), Some(false));
        assert_eq!(h.broadcasts(), vec![Some(false)]);
        h.timers.advance(1000);
        assert_eq!(h.broadcasts(), vec![Some(false), None]);
        assert_eq!(h.kills(0), 1);
        h.timers.advance(2000);
        assert_eq!(h.spawner.count(), 2);
        h.monitor.stop();
    }

    #[test]
    fn delayed_timer_after_sleep_cannot_expose_stale_state() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":true}\n");
        h.timers.advance_without_running(10_000);
        let result = h.monitor.request_state();
        assert_eq!(h.broadcasts(), vec![Some(true), None]);
        assert_eq!(h.kills(0), 1);
        h.timers.advance(1500);
        assert_eq!(result.try_recv().unwrap(), None);
        h.timers.advance(500);
        assert_eq!(h.spawner.count(), 2);
        h.monitor.stop();
    }

    #[test]
    fn stop_resolves_pending_reads_and_cancels_timers() {
        let (h, _) = helper();
        let result = h.monitor.request_state();
        h.monitor.stop();
        assert_eq!(result.try_recv().unwrap(), None);
        assert_eq!(h.kills(0), 1);
        assert_eq!(h.timers.pending(), 0);
        h.spawner.emit(
            0,
            ChildEvent::Exit {
                code: Some(0),
                signal: None,
            },
        );
        h.timers.advance(10_000);
        assert_eq!(h.spawner.count(), 1);
    }

    #[test]
    fn missing_helper_is_retried_without_blocking() {
        let (h, lookups) = lid(None);
        let result = h.monitor.request_state();
        h.timers.advance(1500);
        assert_eq!(result.try_recv().unwrap(), None);
        h.timers.advance(500);
        assert_eq!(lookups.load(std::sync::atomic::Ordering::SeqCst), 2);
        h.monitor.stop();
        assert_eq!(h.timers.pending(), 0);
    }

    #[test]
    fn excessive_output_retires_the_helper() {
        let (h, _) = helper();
        h.monitor.start();
        h.out(0, "{\"lidClosed\":true}\n");
        h.out(0, &"x".repeat(4097));
        assert_eq!(h.broadcasts(), vec![Some(true), None]);
        assert_eq!(h.kills(0), 1);
        h.monitor.stop();
    }
}
