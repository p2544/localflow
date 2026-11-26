//! Soft start/stop sound cues for dictation, synthesized at runtime and
//! played on the default output device (no bundled audio files).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Peak amplitude scale; kept low so cues read as a gentle tick.
const VOLUME: f32 = 0.12;

/// Which moment the cue marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chime {
    /// Recording began (rising).
    Start,
    /// Recording ended (falling).
    Stop,
}

/// Selectable cue sound, chosen in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChimeStyle {
    /// Two quick soft notes (E5/A5), rising on start, falling on stop.
    SoftPop,
    /// One round sine "bloop" whose pitch glides up on start, down on stop.
    #[default]
    Bubble,
    /// A very short, bright tick; higher on start, lower on stop.
    Tick,
}

/// Plays the cue on a background thread; never blocks the caller and
/// silently does nothing if no output device is available.
pub fn play(style: ChimeStyle, chime: Chime) {
    std::thread::spawn(move || {
        if let Err(e) = play_blocking(style, chime) {
            tracing::debug!("chime playback skipped: {e:#}");
        }
    });
}

/// Renders the cue as mono samples at `rate` Hz.
pub fn render(style: ChimeStyle, chime: Chime, rate: u32) -> Vec<f32> {
    let rate = rate as f32;
    match style {
        ChimeStyle::SoftPop => {
            let notes = match chime {
                Chime::Start => [659.25, 880.0],
                Chime::Stop => [880.0, 659.25],
            };
            let (note_len, gap) = (0.075, 0.045);
            let mut out = vec![0.0f32; ((gap + note_len + 0.02) * rate) as usize];
            for (k, freq) in notes.into_iter().enumerate() {
                let offset = (k as f32 * gap * rate) as usize;
                for i in 0..(note_len * rate) as usize {
                    let Some(slot) = out.get_mut(offset + i) else { break };
                    let t = i as f32 / rate;
                    // 4 ms attack, exponential decay: a soft "pop".
                    let env = (t / 0.004).min(1.0) * (-t / 0.022).exp();
                    let phase = TAU * freq * t;
                    // A touch of the octave harmonic warms up the pure sine.
                    *slot += (phase.sin() + 0.18 * (2.0 * phase).sin()) * env * VOLUME;
                }
            }
            out
        }
        ChimeStyle::Bubble => {
            let (f0, f1) = match chime {
                Chime::Start => (520.0, 980.0),
                Chime::Stop => (980.0, 520.0),
            };
            let len = 0.11;
            let n = (len * rate) as usize;
            let mut phase = 0.0f32;
            (0..n)
                .map(|i| {
                    let t = i as f32 / rate;
                    let x = t / len;
                    // Ease-out glide so the pitch settles quickly.
                    let freq = f0 + (f1 - f0) * (1.0 - (1.0 - x).powi(3));
                    phase += TAU * freq / rate;
                    // Raised-sine envelope: smooth in, smooth out, no click.
                    let env = (std::f32::consts::PI * x).sin().powi(2);
                    phase.sin() * env * VOLUME * 1.2
                })
                .collect()
        }
        ChimeStyle::Tick => {
            let freq = match chime {
                Chime::Start => 1760.0,
                Chime::Stop => 1320.0,
            };
            let n = (0.04 * rate) as usize;
            (0..n)
                .map(|i| {
                    let t = i as f32 / rate;
                    let env = (t / 0.001).min(1.0) * (-t / 0.006).exp();
                    let phase = TAU * freq * t;
                    // Inharmonic partial gives a woody, percussive edge.
                    (phase.sin() + 0.35 * (2.76 * phase).sin()) * env * VOLUME
                })
                .collect()
        }
    }
}

fn play_blocking(style: ChimeStyle, chime: Chime) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no default output device"))?;
    let config = device.default_output_config()?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let samples = Arc::new(render(style, chime, rate));
    let duration = Duration::from_secs_f32(samples.len() as f32 / rate as f32);
    let pos = Arc::new(AtomicUsize::new(0));
    let err_fn = |e| tracing::debug!("chime stream error: {e}");

    // Callbacks fill interleaved frames from the mono buffer, then silence.
    fn next(samples: &[f32], pos: &AtomicUsize) -> f32 {
        let i = pos.fetch_add(1, Ordering::Relaxed);
        samples.get(i).copied().unwrap_or(0.0)
    }

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let (s, p) = (samples.clone(), pos.clone());
            device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    for frame in data.chunks_mut(channels) {
                        frame.fill(next(&s, &p));
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let (s, p) = (samples.clone(), pos.clone());
            device.build_output_stream(
                &config.into(),
                move |data: &mut [i16], _| {
                    for frame in data.chunks_mut(channels) {
                        frame.fill((next(&s, &p) * 32767.0) as i16);
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let (s, p) = (samples.clone(), pos.clone());
            device.build_output_stream(
                &config.into(),
                move |data: &mut [u16], _| {
                    for frame in data.chunks_mut(channels) {
                        frame.fill((next(&s, &p) * 32767.0 + 32768.0) as u16);
                    }
                },
                err_fn,
                None,
            )?
        }
        other => anyhow::bail!("unsupported output sample format {other:?}"),
    };
    stream.play()?;
    // Keep the stream alive until the cue (plus device buffer slack) has played.
    std::thread::sleep(duration + Duration::from_millis(150));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: [ChimeStyle; 3] = [ChimeStyle::SoftPop, ChimeStyle::Bubble, ChimeStyle::Tick];

    #[test]
    fn cues_are_short_and_quiet() {
        for style in STYLES {
            for c in [Chime::Start, Chime::Stop] {
                let s = render(style, c, 48_000);
                let secs = s.len() as f32 / 48_000.0;
                assert!(secs < 0.2, "{style:?} too long: {secs}s");
                let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(peak > 0.02 && peak < 0.25, "{style:?} peak {peak}");
            }
        }
    }

    #[test]
    fn cues_fade_out_to_silence() {
        for style in STYLES {
            let s = render(style, Chime::Stop, 44_100);
            let tail = &s[s.len() - 40..];
            assert!(tail.iter().all(|v| v.abs() < 0.01), "{style:?} ends abruptly");
        }
    }

    #[test]
    fn style_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&ChimeStyle::SoftPop).unwrap(), r#""soft_pop""#);
    }
}
