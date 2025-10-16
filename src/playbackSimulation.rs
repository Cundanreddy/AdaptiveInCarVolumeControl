use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::WavReader;
use reqwest::blocking::Client;
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Adaptive gain state with smoothing (attack/release) in dB
struct AdaptiveGain {
    last_gain_db: f32,
    last_update: Instant,
    tau_attack: f32,
    tau_release: f32,
    l_desired_db: f32,
    user_offset_db: f32,
}

impl AdaptiveGain {
    fn new(l_desired_db: f32, tau_attack: f32, tau_release: f32, user_offset_db: f32) -> Self {
        Self {
            last_gain_db: 0.0,
            last_update: Instant::now(),
            tau_attack,
            tau_release,
            l_desired_db,
            user_offset_db,
        }
    }

    fn speed_to_noise(speed_kmh: f32) -> f32 {
        // Tunable model: noise contribution from speed
        let a = 6.0;
        let b = 40.0;
        a * (speed_kmh + 1.0).ln() + b
    }

    /// Compute updated gain based on cabin_db (dB) and speed_kmh
    /// Returns (gain_db_smoothed, gain_lin)
    fn compute_gain(&mut self, cabin_db: f32, speed_kmh: f32) -> (f32, f32) {
        let noise_db = cabin_db.max(Self::speed_to_noise(speed_kmh));
        let mut raw_gain_db = self.l_desired_db - noise_db + self.user_offset_db;
        raw_gain_db = raw_gain_db.clamp(-18.0, 18.0);

        let now = Instant::now();
        let dt = (now - self.last_update).as_secs_f32().max(1e-6);
        self.last_update = now;

        let tau = if raw_gain_db > self.last_gain_db {
            self.tau_attack
        } else {
            self.tau_release
        };
        let alpha = 1.0 - (-dt / tau).exp();
        self.last_gain_db += alpha * (raw_gain_db - self.last_gain_db);

        let gain_lin = 10f32.powf(self.last_gain_db / 20.0);
        (self.last_gain_db, gain_lin)
    }
}

/// Helper: compute RMS -> dB (approx). We add an offset so typical mic RMS maps to reasonable dB.
/// You should calibrate this offset in your environment.
fn rms_to_db(samples: &[f32]) -> f32 {
    let mut sumsq = 0.0f32;
    for &s in samples {
        sumsq += s * s;
    }
    let rms = (sumsq / samples.len() as f32).sqrt().max(1e-9);
    // +94.0 is an arbitrary calibration offset used earlier; adjust per your mic calibration
    20.0 * rms.log10() + 94.0
}

fn main() -> Result<()> {
    // Configuration
    let wav_path = std::env::args().nth(1).unwrap_or("test_audio.wav".to_string());
    let speed_api_url =
        std::env::args().nth(2).unwrap_or("http://127.0.0.1:5005/speed".to_string());
    let poll_period_ms = 150u64; // how often to poll speed API

    println!("Adaptive Volume Rust");
    println!("WAV file: {}", wav_path);
    println!("Speed API URL: {}", speed_api_url);

    // Shared resources
    let playback_queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let gain_lin_shared = Arc::new(Mutex::new(1.0f32)); // latest linear gain to apply
    let speed_shared = Arc::new(Mutex::new(0.0f32)); // km/h

    // Initialize adaptive gain state (controller thread will own it)
    let adaptive_gain = Arc::new(Mutex::new(AdaptiveGain::new(75.0, 0.12, 1.0, 0.0)));

    // 1) Read WAV file into the playback queue (synchronously so we know it's loaded)
    match read_wav_to_queue(&wav_path, &playback_queue) {
        Ok(_) => {
            let qlen = { let q = playback_queue.lock().unwrap(); q.len() };
            println!("WAV loaded into playback queue. queued_samples={}", qlen);
        }
        Err(e) => eprintln!("Failed to load WAV: {e:?}"),
    }

    // 2) Start speed poller thread (blocking reqwest) - updates speed_shared
    {
        let url = speed_api_url.clone();
        let speed_s = speed_shared.clone();
        thread::spawn(move || {
            let client = Client::new();
            loop {
                match client.get(&url).send() {
                    Ok(resp) => {
                        if let Ok(json) = resp.json::<serde_json::Value>() {
                            // Expecting JSON: {"speed": 72.5}  (tunable)
                            if let Some(s) = json.get("speed").and_then(|v| v.as_f64()) {
                                let mut speed_lock = speed_s.lock().unwrap();
                                *speed_lock = s as f32;
                            }
                        }
                    }
                    Err(e) => {
                        // keep trying; don't spam errors
                        eprintln!("Speed poll error: {:?}", e);
                    }
                }
                thread::sleep(Duration::from_millis(poll_period_ms));
            }
        });
    }

    // 3) Start audio host, output stream consumes from playback_queue and applies latest gain
    let host = cpal::default_host();

    let output_device = host
        .default_output_device()
        .expect("No default output device");
    println!("Output device: {}", output_device.name()?);

    let input_device = host
        .default_input_device()
        .expect("No default input device");
    println!("Input device: {}", input_device.name()?);

    let out_config = output_device.default_output_config()?;
    let in_config = input_device.default_input_config()?;
    println!("Output config: {:?}", out_config);
    println!("Input config: {:?}", in_config);

    // Use f32 pipeline for simplicity; convert if devices are other formats
    let sample_rate = out_config.sample_rate().0 as f32;
    let channels_out = out_config.channels() as usize;
    let channels_in = in_config.channels() as usize;

    // Output stream - pulls from playback_queue and applies latest gain
    let played_counter = Arc::new(AtomicUsize::new(0));
    {
        let pq = playback_queue.clone();
        let gain_ref = gain_lin_shared.clone();

        // out_config is a SupportedStreamConfig returned by default_output_config()
        let supported_out: cpal::SupportedStreamConfig = out_config;
        let stream_config: cpal::StreamConfig = supported_out.config();
        let stream = match supported_out.sample_format() {
            cpal::SampleFormat::F32 => build_output_stream::<f32>(
                &output_device,
                &stream_config,
                pq.clone(),
                gain_ref.clone(),
                channels_out,
                played_counter.clone(),
            )?,
            cpal::SampleFormat::I16 => build_output_stream::<i16>(
                &output_device,
                &stream_config,
                pq.clone(),
                gain_ref.clone(),
                channels_out,
                played_counter.clone(),
            )?,
            cpal::SampleFormat::U16 => build_output_stream::<u16>(
                &output_device,
                &stream_config,
                pq.clone(),
                gain_ref.clone(),
                channels_out,
                played_counter.clone(),
            )?,
            _ => unreachable!(),
        };
        stream.play()?;
        println!("Output stream started.");
    }

    // Input stream - collects mic frames and sends them to controller via channel-like arrangement
    // We'll collect small chunks and pass them to the controller thread through a shared buffer
    let controller_queue = Arc::new(Mutex::new(Vec::<f32>::new()));
    {
        let ctrl_q = controller_queue.clone();
        let supported_in: cpal::SupportedStreamConfig = in_config;
        let in_stream_config: cpal::StreamConfig = supported_in.config();
        let input_dev = input_device.clone();
        thread::spawn(move || {
            let err_fn = |err| eprintln!("input stream error: {}", err);
            match supported_in.sample_format() {
                cpal::SampleFormat::F32 => {
                    let stream = input_dev.build_input_stream(
                        &in_stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mut local = ctrl_q.lock().unwrap();
                            local.clear();
                            for frame in data.chunks(in_stream_config.channels as usize) {
                                local.push(frame[0]);
                            }
                        },
                        err_fn,
                        None,
                    );
                    match stream {
                        Ok(s) => {
                            s.play().unwrap();
                            loop { thread::sleep(Duration::from_secs(60)); }
                        }
                        Err(e) => eprintln!("Failed to build input stream: {:?}", e),
                    }
                }
                cpal::SampleFormat::I16 => {
                    let stream = input_dev.build_input_stream(
                        &in_stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let mut local = ctrl_q.lock().unwrap();
                            local.clear();
                            for frame in data.chunks(in_stream_config.channels as usize) {
                                local.push(frame[0] as f32 / i16::MAX as f32);
                            }
                        },
                        err_fn,
                        None,
                    );
                    match stream {
                        Ok(s) => {
                            s.play().unwrap();
                            loop { thread::sleep(Duration::from_secs(60)); }
                        }
                        Err(e) => eprintln!("Failed to build input stream: {:?}", e),
                    }
                }
                cpal::SampleFormat::U16 => {
                    let stream = input_dev.build_input_stream(
                        &in_stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let mut local = ctrl_q.lock().unwrap();
                            local.clear();
                            for frame in data.chunks(in_stream_config.channels as usize) {
                                local.push((frame[0] as f32 - 0.5) * 2.0);
                            }
                        },
                        err_fn,
                        None,
                    );
                    match stream {
                        Ok(s) => {
                            s.play().unwrap();
                            loop { thread::sleep(Duration::from_secs(60)); }
                        }
                        Err(e) => eprintln!("Failed to build input stream: {:?}", e),
                    }
                }
                _ => unreachable!(),
            }
        });
    }

    // Start a small monitor to help diagnose playback (queue length, played samples, current gain)
    {
        let pqm = playback_queue.clone();
        let gm = gain_lin_shared.clone();
        let pc = played_counter.clone();
        thread::spawn(move || {
            let mut last_count = 0usize;
            loop {
                let qlen = { let q = pqm.lock().unwrap(); q.len() };
                let gain = { let g = gm.lock().unwrap(); *g };
                let count = pc.load(Ordering::Relaxed);
                println!("[Monitor] queue_len={} gain={:.3} played_total={} delta={}", qlen, gain, count, count - last_count);
                last_count = count;
                thread::sleep(Duration::from_secs(1));
            }
        });
    }

    // 4) Controller thread: periodically reads controller_queue (mic), speed_shared (speed),
    //    computes gain via AdaptiveGain, and writes linear gain into gain_lin_shared
    {
        let ctrl_q = controller_queue.clone();
        let speed_s = speed_shared.clone();
        let gain_lin_s = gain_lin_shared.clone();
        let adaptive = adaptive_gain.clone();
        thread::spawn(move || {
            // controller runs at ~ 20 Hz (50 ms)
            let interval = Duration::from_millis(50);
            loop {
                let mut mic_samples: Vec<f32> = {
                    let guard = ctrl_q.lock().unwrap();
                    guard.clone()
                };

                if mic_samples.is_empty() {
                    thread::sleep(interval);
                    continue;
                }

                // compute cabin dB from mic samples
                let cabin_db = rms_to_db(&mic_samples);

                // read latest speed
                let speed_kmh = {
                    let s = speed_s.lock().unwrap();
                    *s
                };

                // compute gain
                let (gain_db, gain_lin) = {
                    let mut ag = adaptive.lock().unwrap();
                    ag.compute_gain(cabin_db, speed_kmh)
                };

                // update shared gain_lin for output callback
                {
                    let mut gl = gain_lin_s.lock().unwrap();
                    *gl = gain_lin;
                }

                println!(
                    "[Controller] cabin_db={:.1} dB | speed={:.1} km/h | gain_db={:.2} | gain_lin={:.3}",
                    cabin_db, speed_kmh, gain_db, gain_lin
                );

                thread::sleep(interval);
            }
        });
    }

    // Keep main alive
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

/// Read WAV file samples and push them into the playback queue as f32 samples (mono).
fn read_wav_to_queue(path: &str, queue: &Arc<Mutex<VecDeque<f32>>>) -> Result<()> {
    let f = File::open(path)?;
    let mut reader = WavReader::new(BufReader::new(f))?;
    let spec = reader.spec();
    println!("WAV spec: {:?}", spec);

    let mut samples = Vec::<f32>::new();
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for s in reader.samples::<f32>() {
                let v = s?;
                samples.push(v);
            }
        }
        hound::SampleFormat::Int => {
            let max_amplitude = (1i128 << (spec.bits_per_sample - 1)) as f32;
            for s in reader.samples::<i32>() {
                let v = s?;
                samples.push(v as f32 / max_amplitude);
            }
        }
    }

    // If stereo, convert to mono by taking first channel (interleaved)
    let channels = spec.channels as usize;
    let mut mono: Vec<f32> = Vec::with_capacity(samples.len() / channels);
    if channels == 1 {
        mono = samples;
    } else {
        for chunk in samples.chunks(channels) {
            mono.push(chunk[0]);
        }
    }

    // Push into queue
    {
        let mut q = queue.lock().unwrap();
        for s in mono.into_iter() {
            q.push_back(s);
        }
    }
    Ok(())
}

/// Build output stream for specified sample type T.
/// Pulls samples from playback_queue, applies gain from gain_ref, writes to output buffer.
/// If playback_queue empties, writes silence.
fn build_output_stream<T>(
    output_device: &cpal::Device,
    config: &cpal::StreamConfig,
    playback_queue: Arc<Mutex<VecDeque<f32>>>,
    gain_ref: Arc<Mutex<f32>>,
    channels: usize,
    played_counter: Arc<AtomicUsize>,
) -> Result<cpal::Stream>
where
    T: cpal::Sample + cpal::FromSample<f32> + cpal::SizedSample,
{
    let err_fn = |err| eprintln!("output stream error: {}", err);

    let stream = output_device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            // data is interleaved frames
            let mut q = playback_queue.lock().unwrap();
            let gain = {
                let g = gain_ref.lock().unwrap();
                *g
            };

            for frame in data.chunks_mut(channels) {
                let s = q.pop_front().unwrap_or(0.0f32);
                // Apply gain and soft clip
                let mut out = s * gain;
                // soft clip a bit to avoid hard clipping
                if out > 0.99 {
                    out = 0.99 + (out - 0.99) / (1.0 + (out - 0.99));
                } else if out < -0.99 {
                    out = -0.99 + (out + 0.99) / (1.0 + (-out - 0.99));
                }
                let sample: T = <T as cpal::FromSample<f32>>::from_sample_(out);
                let mut wrote_nonzero = false;
                for ch in frame.iter_mut() {
                    *ch = sample;
                    // detect non-silence (simple): if written sample != 0.0
                    wrote_nonzero = wrote_nonzero || s != 0.0f32;
                }
                if wrote_nonzero {
                    played_counter.fetch_add(frame.len(), Ordering::Relaxed);
                }
            }
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}
