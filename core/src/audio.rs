//! Microphone capture via cpal: any device rate/channels → 16 kHz mono f32.

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Captures mic audio on a cpal stream into a shared buffer until stopped.
pub struct Recorder {
    stream: Option<cpal::Stream>,
    shared: Arc<SharedCapture>,
    source_rate: u32,
    source_channels: u16,
}

struct SharedCapture {
    samples: Mutex<Vec<f32>>,
    /// RMS of the most recent callback block, for the UI waveform.
    level: Mutex<f32>,
    active: AtomicBool,
}

impl Recorder {
    /// Opens the requested (or default) input device and starts capturing.
    pub fn start(device_name: &str) -> Result<Self> {
        let host = cpal::default_host();
        let device = if device_name.is_empty() {
            host.default_input_device()
                .context("no default input device")?
        } else {
            host.input_devices()?
                .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
                .or_else(|| host.default_input_device())
                .context("no matching input device")?
        };

        let config = device
            .default_input_config()
            .context("no default input config")?;
        let source_rate = config.sample_rate().0;
        let source_channels = config.channels();

        let shared = Arc::new(SharedCapture {
            samples: Mutex::new(Vec::with_capacity(source_rate as usize * 30)),
            level: Mutex::new(0.0),
            active: AtomicBool::new(true),
        });

        let cb_shared = shared.clone();
        let channels = source_channels as usize;
        let err_fn = |e| tracing::error!("audio stream error: {e}");

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| push_frames(&cb_shared, data, channels),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    let f: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                    push_frames(&cb_shared, &f, channels);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    let f: Vec<f32> =
                        data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0).collect();
                    push_frames(&cb_shared, &f, channels);
                },
                err_fn,
                None,
            )?,
            other => return Err(anyhow!("unsupported sample format {other:?}")),
        };
        stream.play()?;

        Ok(Self {
            stream: Some(stream),
            shared,
            source_rate,
            source_channels,
        })
    }

    /// Instantaneous input level in [0, 1] for UI metering.
    pub fn level(&self) -> f32 {
        *self.shared.level.lock()
    }

    pub fn duration_secs(&self) -> f32 {
        let n = self.shared.samples.lock().len();
        n as f32 / self.source_rate as f32
    }

    /// Stops capture and returns 16 kHz mono samples.
    pub fn stop(mut self) -> Result<Vec<f32>> {
        self.shared.active.store(false, Ordering::SeqCst);
        // Dropping the stream stops the callback.
        drop(self.stream.take());
        let mono = std::mem::take(&mut *self.shared.samples.lock());
        resample_to_16k(&mono, self.source_rate)
    }

    pub fn source_info(&self) -> (u32, u16) {
        (self.source_rate, self.source_channels)
    }
}

/// Downmixes interleaved frames to mono and appends to the shared buffer.
fn push_frames(shared: &SharedCapture, data: &[f32], channels: usize) {
    if !shared.active.load(Ordering::Relaxed) || channels == 0 {
        return;
    }
    let mut mono = Vec::with_capacity(data.len() / channels);
    for frame in data.chunks_exact(channels) {
        mono.push(frame.iter().sum::<f32>() / channels as f32);
    }
    let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len().max(1) as f32).sqrt();
    *shared.level.lock() = (rms * 8.0).min(1.0);
    shared.samples.lock().extend_from_slice(&mono);
}

/// High-quality sinc resample of mono audio to 16 kHz.
pub fn resample_to_16k(mono: &[f32], source_rate: u32) -> Result<Vec<f32>> {
    if source_rate == TARGET_SAMPLE_RATE || mono.is_empty() {
        return Ok(mono.to_vec());
    }
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    let chunk = 1024;
    let mut resampler = SincFixedIn::<f32>::new(
        TARGET_SAMPLE_RATE as f64 / source_rate as f64,
        2.0,
        params,
        chunk,
        1,
    )?;
    let mut out = Vec::with_capacity(
        (mono.len() as u64 * TARGET_SAMPLE_RATE as u64 / source_rate as u64) as usize + chunk,
    );
    let mut pos = 0;
    while pos + chunk <= mono.len() {
        let res = resampler.process(&[&mono[pos..pos + chunk]], None)?;
        out.extend_from_slice(&res[0]);
        pos += chunk;
    }
    if pos < mono.len() {
        let res = resampler.process_partial(Some(&[&mono[pos..]]), None)?;
        out.extend_from_slice(&res[0]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_halves_sample_count_from_32k() {
        let src: Vec<f32> = (0..32_000)
            .map(|i| (i as f32 * 0.05).sin() * 0.5)
            .collect();
        let out = resample_to_16k(&src, 32_000).unwrap();
        let expect = 16_000f32;
        assert!(
            (out.len() as f32 - expect).abs() / expect < 0.05,
            "got {} samples",
            out.len()
        );
    }

    #[test]
    fn resample_noop_at_16k() {
        let src = vec![0.1f32; 1600];
        let out = resample_to_16k(&src, 16_000).unwrap();
        assert_eq!(out.len(), src.len());
    }
}
