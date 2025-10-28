// main.rs
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::thread::sleep;
use std::time::Duration;

use rodio::{buffer::SamplesBuffer, Decoder, OutputStreamBuilder, Sink, Source};

mod adaptive_gain;
use adaptive_gain::{
    db_to_lin, mock_get_cabin_noise_db, mock_get_speed_kmh, speed_to_noise, Smoother, L_DESIRED_DB,
    USER_OFFSET_DB, BASE_NOISE_DB, GAIN_SENSITIVITY,
};

// Blocking HTTP fetch (returns None on any error)
fn fetch_remote_state(url: &str) -> Option<(f32, f32)> {
    // note: reqwest + serde_json are required in Cargo.toml
    let resp = reqwest::blocking::get(url).ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().ok()?;
    let cabin_db = json.get("cabin_db")?.as_f64()? as f32;
    let speed_kmh = json.get("speed_kmh")?.as_f64()? as f32;
    Some((cabin_db, speed_kmh))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ---------- config ----------
    let input_path = "test_audio.wav";
    let args: Vec<String> = env::args().collect();
    let auto_mode = args.iter().any(|a| a == "--auto");

    if !std::path::Path::new(input_path).exists() {
        return Err(format!(
            "Input file '{}' not found. Put a WAV file at this path or change `input_path`.",
            input_path
        )
        .into());
    }

    // Remote UI endpoint (used in manual mode to fetch cabin_db/speed each chunk)
    let remote_url =
        std::env::var("SPEED_UI_URL").unwrap_or_else(|_| "http://127.0.0.1:5005/state".into());

    // ---------- audio init ----------
    let stream_handle = OutputStreamBuilder::open_default_stream()?;
    let sink = Sink::connect_new(&stream_handle.mixer());
    let sink = std::sync::Arc::new(sink);

    // ---------- decode and collect samples (f32) ----------
    // We must read all samples since we need random access by chunk.
    // Decoder yields f32 samples in [-1.0,1.0] when converted.
    let file = BufReader::new(File::open(input_path)?);
    let source = Decoder::new(file)?;
    let sample_rate = source.sample_rate();
    let channels = source.channels();
    let samples_f32: Vec<f32> = source.collect();

    // chunk_frames = ~0.1s
    let chunk_frames = (sample_rate as usize / 10).max(1);
    let chunk_size = chunk_frames * channels as usize; // interleaved samples per chunk
    let total_chunks = (samples_f32.len() + chunk_size - 1) / chunk_size;

    // Smoother for gain in dB: attack=0.1s, release=1.0s (as used previously)
    let mut smoother = Smoother::new(0.0, 0.1, 1.0);

    // Time tracking for mocks (auto mode)
    let mut t = 0.0_f32;
    let dt = chunk_frames as f32 / sample_rate as f32;

    println!(
        "Starting playback: '{}' ({} Hz, {} channels) — mode: {}",
        input_path,
        sample_rate,
        channels,
        if auto_mode { "AUTO (mocked)" } else { "MANUAL (remote UI poll)" }
    );

    // main chunk loop — compute gain per chunk, apply, append, and sleep to pace playback
    for i in 0..total_chunks {
        // fetch inputs: either from mocks (auto) or remote UI (manual)
        let (cabin_db, speed_kmh) = if auto_mode {
            (mock_get_cabin_noise_db(t), mock_get_speed_kmh(t))
        } else {
            match fetch_remote_state(&remote_url) {
                Some((c, s)) => (c, s),
                None => {
                    eprintln!(
                        "[warn] failed to fetch remote state from {}, using last-known mock values",
                        remote_url
                    );
                    (mock_get_cabin_noise_db(t), mock_get_speed_kmh(t))
                }
            }
        };

        // convert speed to noise model and combine with cabin_db (we use max as before)
        let speed_noise_db = speed_to_noise(speed_kmh);
        let noise_db: f32 = cabin_db.max(speed_noise_db);

    // compute raw gain in dB and clamp it
    // Previous behaviour tried to maintain a target playback level: gain = L_DESIRED - noise.
    // To make volume increase with speed/noise we compute a baseline gain at a
    // reference (quiet cabin) and then add a scaled boost proportional to
    // how much the measured noise is above that baseline.
    let baseline_noise_db = BASE_NOISE_DB;
    let sensitivity = GAIN_SENSITIVITY; // how many dB playback gain per 1 dB noise increase
    let base_gain_db = L_DESIRED_DB - baseline_noise_db;
    let mut gain_db_raw = base_gain_db + sensitivity * (noise_db - baseline_noise_db) + USER_OFFSET_DB;
    // keep gain within reasonable bounds to avoid extreme boosting
    gain_db_raw = gain_db_raw.max(-24.0).min(24.0);

        // smooth and convert to linear
        let gain_db = smoother.step(gain_db_raw);
        let gain_lin = db_to_lin(gain_db);

        // slice chunk, apply gain and clamp to [-1.0,1.0]
        let start = i * chunk_size;
        let end = ((i + 1) * chunk_size).min(samples_f32.len());
        if start >= end {
            break;
        }

        let mut chunk = Vec::with_capacity(end - start);
        for &s in &samples_f32[start..end] {
            chunk.push((s * gain_lin).clamp(-1.0_f32, 1.0_f32));
        }

        // create samples buffer (interleaved samples) and append
        let src = SamplesBuffer::new(channels, sample_rate, chunk);
        sink.append(src);

        // Print live status (kept short)
        println!(
            "[{:>6.2}s] speed={:>5.1} km/h, cabin={:>5.1} dB, gain_db={:>+5.2} dB, gain_lin={:.3}",
            t, speed_kmh, noise_db, gain_db, gain_lin
        );

        // advance time for mocks & pace appending to avoid queue blowout
        t += dt;
        sleep(Duration::from_secs_f32(dt));
    }

    // Wait until playback ends
    sink.sleep_until_end();
    println!("✅ Playback finished.");
    Ok(())
}
mod audio_playback6_test;