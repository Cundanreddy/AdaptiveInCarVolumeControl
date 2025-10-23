use std::thread::sleep;
use std::{fs::File, io::BufReader, time::Duration};
use std::env;
use rodio::{Decoder, Sink, Source, OutputStreamBuilder};

mod adaptive_gain;
use adaptive_gain::{
    
    L_DESIRED_DB,
    USER_OFFSET_DB,
    speed_to_noise,
    Smoother,
    db_to_lin,
    mock_get_cabin_noise_db,
    mock_get_speed_kmh,
};

// Fetch state from a local Python UI server (blocking). Expected JSON: { "cabin_db": 60.0, "speed_kmh": 70.0 }
fn fetch_remote_state(url: &str) -> Option<(f32, f32)> {
    // Use blocking reqwest (reqwest is present in Cargo.toml)
    let resp = match reqwest::blocking::get(url) {
        Ok(r) => r,
        Err(_) => return None,
    };
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = match resp.json() {
        Ok(j) => j,
        Err(_) => return None,
    };
    let cabin_db = json.get("cabin_db").and_then(|v| v.as_f64()).map(|v| v as f32);
    let speed_kmh = json.get("speed_kmh").and_then(|v| v.as_f64()).map(|v| v as f32);
    match (cabin_db, speed_kmh) {
        (Some(c), Some(s)) => Some((c, s)),
        _ => None,
    }
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Input WAV file
    let input_path = "test_audio.wav";

    // Read CLI argument
    let args: Vec<String> = env::args().collect();
    let auto_mode = args.iter().any(|a| a == "--auto");

    if !std::path::Path::new(input_path).exists() {
        return Err(format!(
            "Input file '{}' not found. Put a WAV file at this path or change `input_path`.",
            input_path
        ).into());
    }

    // Initialize audio output stream
    let stream_handle = OutputStreamBuilder::open_default_stream()?;
    let sink = Sink::connect_new(&stream_handle.mixer());

    let mut smoother = Smoother::new(0.0, 0.1, 1.0); // tau_attack=0.1s, tau_release=1s

    // Load and decode WAV
    let file = BufReader::new(File::open(input_path)?);
    let source = Decoder::new(file)?;

    if !auto_mode {
        // Manual gain
        let remote_url = std::env::var("SPEED_UI_URL").unwrap_or_else(|_| "http://127.0.0.1:5005/state".to_string());
        let (cabin_db, speed) = if let Some((c, s)) = fetch_remote_state(&remote_url) {
            (c, s)
        } else {
            (60.0, 40.0)
        };
        let speed_noise = speed_to_noise(speed);
        let noise_db: f32 = cabin_db.max(speed_noise);

         // 2) compute raw gain dB
        let gain_db_raw = L_DESIRED_DB - noise_db + USER_OFFSET_DB;

        // clamp gain_db within reasonable bounds
        let gain_db_raw = gain_db_raw.max(-24.0).min(24.0);

        // 3) smooth
        let gain_db = smoother.step(gain_db_raw);

        // 4) convert to linear
        let gain_lin = db_to_lin(gain_db);

        let gain = gain_lin;
        println!("▶ Playing '{}' with variable gain {:.2}", input_path, gain);
        sink.append(source.amplify(gain_lin));
        sink.sleep_until_end();
        println!("✅ Playback finished.");
        return Ok(());
    }

    // AUTO MODE
    println!("🚗 Auto mode enabled — simulating speed/noise and adaptive gain...");

    // Split source into small chunks to allow dynamic gain control
    // Use the decoder's sample rate and channel count to avoid playback speed mismatch
    let sample_rate = source.sample_rate(); // u32
    let sample_rate_usize = sample_rate as usize;
    let channels = source.channels() as usize; // number of interleaved channels
    let samples_f32: Vec<f32> = source.collect();

    // chunk_frames is number of frames per chunk (not samples). Multiply by channels to get sample count.
    let chunk_frames = (sample_rate_usize / 10).max(1); // ~0.1s worth of frames
    let chunk_size = chunk_frames * channels; // samples per chunk (interleaved)
    let total_chunks = (samples_f32.len() + chunk_size - 1) / chunk_size;

    let mut t = 0.0f32;
    let dt = chunk_frames as f32 / sample_rate_usize as f32; // duration per chunk (in seconds)
    for i in 0..total_chunks {
        

        let cabin_db = mock_get_cabin_noise_db(t);
        let speed = mock_get_speed_kmh(t);
        let speed_noise = speed_to_noise(speed);
        let noise_db: f32 = cabin_db.max(speed_noise);

         // 2) compute raw gain dB
        let gain_db_raw = L_DESIRED_DB - noise_db + USER_OFFSET_DB;

        // clamp gain_db within reasonable bounds
        let gain_db_raw = gain_db_raw.max(-24.0).min(24.0);

        // 3) smooth
        let gain_db = smoother.step(gain_db_raw);

        // 4) convert to linear
        let gain_lin = db_to_lin(gain_db);

        let start = i * chunk_size;
        let end = ((i + 1) * chunk_size).min(samples_f32.len());
        if start >= end { break; }



        // Decoder provides f32 samples in [-1.0, 1.0]. Apply gain and clamp in that domain.
        let chunk = samples_f32[start..end].iter()
            .map(|&s| (s * gain_lin).clamp(-1.0_f32, 1.0_f32))
            .collect::<Vec<f32>>();

        let src = rodio::buffer::SamplesBuffer::new(channels as u16, sample_rate, chunk);
        sink.append(src);

        println!(
            "Speed: {:>5.1} km/h | Noise: {:>5.1} dB | Gain: {:.2} | time:{:.2}s",
            speed, noise_db, gain_lin,t
        );
        t += dt;
        sleep(Duration::from_secs_f32(dt)); // simulate real time
    }

    sink.sleep_until_end();
    println!("✅ Auto playback finished.");
    Ok(())
}
