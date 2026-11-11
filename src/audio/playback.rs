//! Shared in-process output for cues and memory-only microphone tests.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use parking_lot::Mutex;

use super::{AudioError, wav::resample};

#[derive(Default)]
struct Status {
    position: AtomicUsize,
    length: AtomicUsize,
    done: AtomicBool,
    error: Mutex<Option<AudioError>>,
}

pub struct Playback {
    cancel: mpsc::Sender<()>,
    status: Arc<Status>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Playback {
    pub fn start(samples: Arc<Vec<f32>>, rate: u32, exclusive: bool) -> Result<Self, AudioError> {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Status::default());
        let progress = status.clone();
        let thread = std::thread::Builder::new()
            .name("whisper-playback".into())
            .spawn(move || {
                // Test playback and input share a lease. Cancellation can be
                // requested while a previous stream is still closing.
                let _lease = exclusive.then(|| super::capture::AUDIO_INPUT_LEASE.lock());
                if rx.try_recv().is_ok() {
                    progress.done.store(true, Ordering::Release);
                    return;
                }
                if let Err(error) = play(&samples, rate, &rx, progress.clone()) {
                    *progress.error.lock() = Some(error);
                }
                progress.done.store(true, Ordering::Release);
            })
            .map_err(|e| AudioError::Output(e.to_string()))?;
        Ok(Self {
            cancel: tx,
            status,
            thread: Some(thread),
        })
    }

    pub fn progress(&self) -> f32 {
        let length = self.status.length.load(Ordering::Relaxed);
        if length == 0 {
            return 0.;
        }
        (self.status.position.load(Ordering::Relaxed) as f32 / length as f32).min(1.)
    }

    pub fn is_done(&self) -> bool {
        self.status.done.load(Ordering::Acquire)
    }

    pub fn error(&self) -> Option<AudioError> {
        self.status.error.lock().clone()
    }

    pub fn wait(mut self) -> Result<(), AudioError> {
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| AudioError::Output("output worker failed".into()))?;
        }
        self.error().map_or(Ok(()), Err)
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
    }
}

fn play(
    samples: &[f32],
    rate: u32,
    cancel: &mpsc::Receiver<()>,
    status: Arc<Status>,
) -> Result<(), AudioError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| AudioError::Output("no output device".into()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::Output(e.to_string()))?;
    let config = supported.config();
    let samples = resample(samples, rate, config.sample_rate);
    status.length.store(samples.len(), Ordering::Relaxed);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, samples, status.clone()),
        SampleFormat::F64 => build::<f64>(&device, config, samples, status.clone()),
        SampleFormat::I16 => build::<i16>(&device, config, samples, status.clone()),
        SampleFormat::I32 => build::<i32>(&device, config, samples, status.clone()),
        SampleFormat::U16 => build::<u16>(&device, config, samples, status.clone()),
        SampleFormat::U8 => build::<u8>(&device, config, samples, status.clone()),
        other => Err(AudioError::Output(format!(
            "unsupported sample format {other:?}"
        ))),
    }?;
    stream
        .play()
        .map_err(|e| AudioError::Output(e.to_string()))?;
    loop {
        match cancel.recv_timeout(Duration::from_millis(10)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if status.error.lock().is_some() {
            break;
        }
        if status.position.load(Ordering::Relaxed) >= status.length.load(Ordering::Relaxed) {
            // The callback has filled the last output buffer; let it drain.
            let _ = cancel.recv_timeout(Duration::from_millis(80));
            break;
        }
    }
    drop(stream);
    Ok(())
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    samples: Vec<f32>,
    status: Arc<Status>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let mut position = 0;
    let progress = status.clone();
    device
        .build_output_stream::<T, _, _>(
            config,
            move |out, _| {
                for frame in out.chunks_mut(channels) {
                    let value = samples.get(position).copied().unwrap_or(0.);
                    position += 1;
                    for slot in frame {
                        *slot = T::from_sample_(value);
                    }
                }
                progress
                    .position
                    .store(position.min(samples.len()), Ordering::Relaxed);
            },
            move |error| {
                *status.error.lock() = Some(AudioError::Output(error.to_string()));
            },
            None,
        )
        .map_err(|e| AudioError::Output(e.to_string()))
}
