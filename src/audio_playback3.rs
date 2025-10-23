use std::thread::sleep;
use std::{fs::File, io::BufReader, time::Duration};
use std::env;
use rodio::{Decoder, Sink, Source, OutputStreamBuilder};

mod adaptive_gain;
use adaptive_gain::{
    SAMPLE_RATE,
    CHUNK_SAMPLES,
    L_DESIRED_DB,
    USER_OFFSET_DB,
    speed_to_noise,
    Smoother,
    db_to_lin,
    mock_get_cabin_noise_db,
    mock_get_speed_kmh,
};


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

    // Load and decode WAV
    let file = BufReader::new(File::open(input_path)?);
    let source = Decoder::new(file)?;

    if !auto_mode {
        // Manual gain
        
        let gain = 1.5;
        println!("▶ Playing '{}' with fixed gain {:.2}", input_path, gain);
        sink.append(source.amplify(gain));
        sink.sleep_until_end();
        println!("✅ Playback finished.");
        return Ok(());
    }

    // AUTO MODE
    println!("🚗 Auto mode enabled — simulating speed/noise and adaptive gain...");

    // Split source into small chunks to allow dynamic gain control
    let samples_f32: Vec<f32> = source.collect();
    let chunk_size = (SAMPLE_RATE / 10).max(1); // ~0.1s chunks
    let total_chunks = (samples_f32.len() + chunk_size - 1) / chunk_size;

    let mut smoother = Smoother::new(0.0, 0.1, 1.0); // tau_attack=0.1s, tau_release=1s
    let mut t = 0.0f32;
    let dt = CHUNK_SAMPLES as f32 / SAMPLE_RATE as f32;
    for i in 0..total_chunks {
        // Simulate changing speed and noise
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

        let src = rodio::buffer::SamplesBuffer::new(1, SAMPLE_RATE as u32, chunk);
        sink.append(src);

        println!(
            "Speed: {:>5.1} km/h | Noise: {:>5.1} dB | Gain: {:.2}",
            speed, noise_db, gain_lin
        );
        t += dt;
        sleep(Duration::from_secs_f32(dt)); // simulate real time
    }

    sink.sleep_until_end();
    println!("✅ Auto playback finished.");
    Ok(())
}
