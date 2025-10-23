use std::fs::File;
use std::io::BufReader;
use rodio::{Decoder, Source};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Input WAV file
    let input_path = "test_audio.wav";
    let gain: f32 = 1.5; // 1.0 = normal, >1.0 = louder, <1.0 = quieter

    // Friendly check for the input file so the user sees a clear error instead of a silent exit code
    if !std::path::Path::new(input_path).exists() {
        return Err(format!("Input file '{}' not found. Put a WAV file at this path or change `input_path`.", input_path).into());
    }

    // Initialize audio output stream using the builder API provided by this version of rodio
    let stream_handle = rodio::OutputStreamBuilder::open_default_stream()?;
    let sink = rodio::Sink::connect_new(&stream_handle.mixer());

    // Load and decode WAV
    let file = BufReader::new(File::open(input_path)?);
    let source = Decoder::new(file)?;

    // Apply gain and play
    let amplified = source.amplify(gain);
    sink.append(amplified);

    println!("▶ Playing '{}' with gain {:.2} ...", input_path, gain);

    // Wait until playback finishes
    sink.sleep_until_end();
    println!("✅ Playback finished.");
    Ok(())
}
