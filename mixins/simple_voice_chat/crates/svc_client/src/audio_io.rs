//! Real audio devices via cpal: microphone capture into 48 kHz mono
//! frames and playback of mixed 48 kHz mono frames. Pure conversion
//! helpers are unit tested; the stream paths need hardware and are
//! exercised on a real client.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960;

type Queue = Arc<Mutex<VecDeque<i16>>>;

/// Average interleaved multi-channel samples down to mono.
pub fn mono_of(interleaved: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|ch| {
            let sum: i64 = ch.iter().map(|s| *s as i64).sum();
            (sum / ch.len() as i64) as i16
        })
        .collect()
}

/// Linear resampling to exactly `out_len` samples.
pub fn resample_exact(samples: &[i16], from: u32, to: u32, out_len: usize) -> Vec<i16> {
    if out_len == 0 {
        return Vec::new();
    }
    if samples.is_empty() {
        return vec![0i16; out_len];
    }
    if from == to && samples.len() == out_len {
        return samples.to_vec();
    }
    (0..out_len)
        .map(|i| {
            let t = i as f64 * from as f64 / to as f64;
            let i0 = (t.floor() as usize).min(samples.len() - 1);
            let i1 = (i0 + 1).min(samples.len() - 1);
            let f = (t - i0 as f64) as f32;
            let a = samples[i0] as f32;
            (a + (samples[i1] as f32 - a) * f) as i16
        })
        .collect()
}

fn open_in<T: cpal::SizedSample>(
    dev: &cpal::Device,
    cfg: &cpal::SupportedStreamConfig,
    q: Queue,
    conv: impl Fn(T) -> i16 + Send + 'static,
) -> Result<cpal::Stream, String> {
    dev.build_input_stream(
        &cfg.config(),
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut q = q.lock().unwrap();
            for s in data {
                q.push_back(conv(*s));
            }
        },
        move |_| {},
        None,
    )
    .map_err(|e| e.to_string())
}

#[inline]
pub fn soft_limit(x: f32) -> f32 {
    const THRESHOLD: f32 = 0.7;
    if x > THRESHOLD {
        let over = x - THRESHOLD;
        THRESHOLD + (1.0 - THRESHOLD) * (over / (1.0 - THRESHOLD + over)).min(1.0)
    } else if x < -THRESHOLD {
        let over = -x - THRESHOLD;
        -(THRESHOLD + (1.0 - THRESHOLD) * (over / (1.0 - THRESHOLD + over)).min(1.0))
    } else {
        x
    }
}

pub struct SpeakerChannel {
    pub left: VecDeque<i16>,
    pub right: VecDeque<i16>,
    pub prebuffering: bool,
    pub last_active: Instant,
}

impl SpeakerChannel {
    pub fn new() -> Self {
        Self {
            left: VecDeque::new(),
            right: VecDeque::new(),
            prebuffering: true,
            last_active: Instant::now(),
        }
    }
}

type ChannelMap = Arc<Mutex<HashMap<u128, SpeakerChannel>>>;

fn open_out<T: cpal::SizedSample + Send + 'static>(
    dev: &cpal::Device,
    cfg: &cpal::SupportedStreamConfig,
    channels: ChannelMap,
    unconv: impl Fn(f32) -> T + Send + 'static + Copy,
    silence: T,
) -> Result<cpal::Stream, String> {
    let channels_count = cfg.channels() as usize;
    let sample_rate = cfg.sample_rate().0;
    // 40ms prebuffering threshold (2 frames of 20ms)
    let prebuffer_threshold = (sample_rate as usize * 40) / 1000;

    dev.build_output_stream(
        &cfg.config(),
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            if channels_count == 0 {
                return;
            }
            let frames = data.len() / channels_count;
            let mut channels_lock = channels.lock().unwrap();

            if channels_lock.is_empty() {
                for s in data.iter_mut() {
                    *s = silence;
                }
                return;
            }

            for f in 0..frames {
                let mut sum_l = 0.0f32;
                let mut sum_r = 0.0f32;

                for ch in channels_lock.values_mut() {
                    if ch.prebuffering {
                        if ch.left.len() >= prebuffer_threshold {
                            ch.prebuffering = false;
                        }
                    }

                    if !ch.prebuffering {
                        if ch.left.is_empty() {
                            ch.prebuffering = true;
                        } else {
                            let l = ch.left.pop_front().unwrap_or(0);
                            let r = ch.right.pop_front().unwrap_or(0);
                            sum_l += l as f32 / 32768.0;
                            sum_r += r as f32 / 32768.0;
                        }
                    }
                }

                let out_l = soft_limit(sum_l);
                let out_r = soft_limit(sum_r);

                let base = f * channels_count;
                if channels_count >= 2 {
                    data[base] = unconv(out_l);
                    data[base + 1] = unconv(out_r);
                    for i in 2..channels_count {
                        data[base + i] = unconv((out_l + out_r) * 0.5);
                    }
                } else {
                    data[base] = unconv((out_l + out_r) * 0.5);
                }
            }
        },
        move |_| {},
        None,
    )
    .map_err(|e| e.to_string())
}

/// Enumerates available input (microphone) devices.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                names.push(name);
            }
        }
    }
    names
}

/// Enumerates available output (speaker/headphone) devices.
pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.output_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                names.push(name);
            }
        }
    }
    names
}

fn find_input_device(host: &cpal::Host, preferred: Option<&str>) -> Result<(cpal::Device, String), String> {
    if let Some(target) = preferred {
        let target = target.trim();
        if !target.is_empty() && !target.eq_ignore_ascii_case("default") {
            if let Ok(devices) = host.input_devices() {
                let mut best_match = None;
                for d in devices {
                    if let Ok(name) = d.name() {
                        if name.eq_ignore_ascii_case(target) {
                            return Ok((d, name));
                        }
                        if name.to_lowercase().contains(&target.to_lowercase()) && best_match.is_none() {
                            best_match = Some((d, name));
                        }
                    }
                }
                if let Some(m) = best_match {
                    return Ok(m);
                }
            }
        }
    }
    let dev = host.default_input_device().ok_or("no default input device found")?;
    let name = dev.name().unwrap_or_else(|_| "Default Microphone".into());
    Ok((dev, name))
}

fn find_output_device(host: &cpal::Host, preferred: Option<&str>) -> Result<(cpal::Device, String), String> {
    if let Some(target) = preferred {
        let target = target.trim();
        if !target.is_empty() && !target.eq_ignore_ascii_case("default") {
            if let Ok(devices) = host.output_devices() {
                let mut best_match = None;
                for d in devices {
                    if let Ok(name) = d.name() {
                        if name.eq_ignore_ascii_case(target) {
                            return Ok((d, name));
                        }
                        if name.to_lowercase().contains(&target.to_lowercase()) && best_match.is_none() {
                            best_match = Some((d, name));
                        }
                    }
                }
                if let Some(m) = best_match {
                    return Ok(m);
                }
            }
        }
    }
    let dev = host.default_output_device().ok_or("no default output device found")?;
    let name = dev.name().unwrap_or_else(|_| "Default Speaker".into());
    Ok((dev, name))
}

/// The microphone: captures whatever format the device gives, converts to
/// mono i16, and hands out 20 ms frames at 48 kHz on demand.
pub struct MicStream {
    _stream: cpal::Stream,
    queue: Queue,
    src_rate: u32,
    src_ch: usize,
    mono: Vec<i16>,
    need: usize,
    device_name: String,
}

impl MicStream {
    pub fn open(preferred: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let (dev, device_name) = find_input_device(&host, preferred)?;
        let cfg = dev
            .default_input_config()
            .map_err(|e| format!("input config for '{}': {}", device_name, e))?;
        let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::I16 => open_in::<i16>(&dev, &cfg, queue.clone(), |s| s)?,
            cpal::SampleFormat::F32 => {
                open_in::<f32>(&dev, &cfg, queue.clone(), |s| {
                    (s.clamp(-1.0, 1.0) * 32767.0) as i16
                })?
            }
            cpal::SampleFormat::U16 => {
                open_in::<u16>(&dev, &cfg, queue.clone(), |s| (s as i32 - 32768) as i16)?
            }
            other => return Err(format!("unsupported mic sample format {:?}", other)),
        };
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            queue,
            src_rate: cfg.sample_rate().0,
            src_ch: cfg.channels() as usize,
            mono: Vec::new(),
            need: ((FRAME_SAMPLES as u64 * cfg.sample_rate().0 as u64 + 47_999) / 48_000) as usize,
            device_name,
        })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// The next 20 ms frame at 48 kHz mono, or None until enough audio is in.
    pub fn poll_frame(&mut self) -> Option<Vec<i16>> {
        let raw: Vec<i16> = self.queue.lock().unwrap().drain(..).collect();
        self.mono.extend(mono_of(&raw, self.src_ch));
        // Drop stale audio so a paused reader never sends minutes-old voice.
        while self.mono.len() > self.need * 12 {
            self.mono.drain(..self.need);
        }
        if self.mono.len() < self.need {
            return None;
        }
        let chunk: Vec<i16> = self.mono.drain(..self.need).collect();
        Some(resample_exact(
            &chunk,
            self.src_rate,
            SAMPLE_RATE,
            FRAME_SAMPLES,
        ))
    }
}

/// The speaker: manages per-speaker channels, converts to the device's own
/// rate and channel count, and mixes all active speakers concurrently with soft limiting.
pub struct Speaker {
    _stream: cpal::Stream,
    channels: ChannelMap,
    dst_rate: u32,
    _dst_ch: usize,
    device_name: String,
}

impl Speaker {
    pub fn open(preferred: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let (dev, device_name) = find_output_device(&host, preferred)?;
        let cfg = dev
            .default_output_config()
            .map_err(|e| format!("output config for '{}': {}", device_name, e))?;
        let channels: ChannelMap = Arc::new(Mutex::new(HashMap::new()));
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::I16 => open_out::<i16>(&dev, &cfg, channels.clone(), |f| {
                (f * 32767.0).clamp(-32768.0, 32767.0) as i16
            }, 0)?,
            cpal::SampleFormat::F32 => {
                open_out::<f32>(&dev, &cfg, channels.clone(), |f| f.clamp(-1.0, 1.0), 0.0)?
            }
            cpal::SampleFormat::U16 => open_out::<u16>(&dev, &cfg, channels.clone(), |f| {
                ((f * 32767.0) as i32 + 32768).clamp(0, u16::MAX as i32) as u16
            }, 32768)?,
            other => return Err(format!("unsupported speaker sample format {:?}", other)),
        };
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            channels,
            dst_rate: cfg.sample_rate().0,
            _dst_ch: cfg.channels() as usize,
            device_name,
        })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Queue one frame of 48 kHz mono PCM for playback (e.g. mic test loopback).
    pub fn play(&self, frame: &[i16]) {
        let mut stereo = Vec::with_capacity(frame.len() * 2);
        for &s in frame {
            stereo.push(s);
            stereo.push(s);
        }
        self.play_stereo_channel(0, &stereo);
    }

    /// Queue one frame of 48 kHz interleaved stereo PCM [L, R, L, R...] for a default channel.
    pub fn play_stereo(&self, frame_stereo: &[i16]) {
        self.play_stereo_channel(0, frame_stereo);
    }

    /// Queue one frame of 48 kHz interleaved stereo PCM [L, R, L, R...] for a specific speaker channel.
    pub fn play_stereo_channel(&self, sender: u128, frame_stereo: &[i16]) {
        let pairs = frame_stereo.len() / 2;
        if pairs == 0 {
            return;
        }
        let out_pairs =
            (pairs as u64 * self.dst_rate as u64 / SAMPLE_RATE as u64) as usize;

        // Split L and R, resample independently to dst_rate
        let mut left_in = Vec::with_capacity(pairs);
        let mut right_in = Vec::with_capacity(pairs);
        for i in 0..pairs {
            left_in.push(frame_stereo[i * 2]);
            right_in.push(frame_stereo[i * 2 + 1]);
        }
        let up_left = resample_exact(&left_in, SAMPLE_RATE, self.dst_rate, out_pairs);
        let up_right = resample_exact(&right_in, SAMPLE_RATE, self.dst_rate, out_pairs);

        let now = Instant::now();
        let mut ch_map = self.channels.lock().unwrap();

        // Evict idle channels inactive for > 3 seconds
        ch_map.retain(|_, ch| now.duration_since(ch.last_active) < std::time::Duration::from_secs(3));

        let ch = ch_map.entry(sender).or_insert_with(SpeakerChannel::new);
        ch.last_active = now;
        ch.left.extend(up_left);
        ch.right.extend(up_right);

        // Cap channel queue at ~200ms of audio to prevent delay accumulation
        let cap = (self.dst_rate as usize * 200) / 1000;
        if ch.left.len() > cap {
            let excess = ch.left.len() - cap;
            ch.left.drain(..excess);
            ch.right.drain(..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_identity_at_native_rate() {
        let x: Vec<i16> = (0..960).map(|i| (i % 100) as i16).collect();
        assert_eq!(resample_exact(&x, 48_000, 48_000, 960), x);
    }

    #[test]
    fn resampling_halves_a_24k_stream() {
        let x: Vec<i16> = (0..960).map(|i| i as i16).collect();
        let down = resample_exact(&x, 48_000, 24_000, 480);
        assert_eq!(down.len(), 480);
        assert_eq!(down[0], 0);
        assert_eq!(down[1], 2);
    }

    #[test]
    fn mono_downmix_averages_channels() {
        let stereo = vec![100, -100, 200, -200];
        assert_eq!(mono_of(&stereo, 2), vec![0, 0]);
        assert_eq!(mono_of(&stereo, 1), stereo);
    }

    #[test]
    fn resample_of_empty_is_silence() {
        assert_eq!(resample_exact(&[], 48_000, 48_000, 10), vec![0i16; 10]);
    }

    #[test]
    fn soft_limit_linear_within_threshold() {
        assert_eq!(soft_limit(0.0), 0.0);
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.5), -0.5);
        assert_eq!(soft_limit(0.7), 0.7);
        assert_eq!(soft_limit(-0.7), -0.7);
    }

    #[test]
    fn soft_limit_compresses_peaks_smoothly() {
        let v1 = soft_limit(1.0);
        assert!(v1 < 1.0);
        assert!(v1 > 0.7);
        let v5 = soft_limit(5.0);
        assert!(v5 <= 1.0);
        let vm5 = soft_limit(-5.0);
        assert!(vm5 >= -1.0);
    }
}
