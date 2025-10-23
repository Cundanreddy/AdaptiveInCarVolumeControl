use hound;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Input and output files
    let input_path = "test_audio.wav";
    let output_path = "output_gain.wav";
    let gain: f32 = 1.5; // Increase volume by 50%, can also be < 1.0 for attenuation

    // Open the input WAV file
    let mut reader = hound::WavReader::open(input_path)?;
    let spec = reader.spec(); // Save WAV format (sample rate, bits, channels, etc.)

    // Create the output WAV file
    let mut writer = hound::WavWriter::create(output_path, spec)?;

    // Process each sample
    match spec.sample_format {
        hound::SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample - 1)) - 1;
            for sample in reader.samples::<i32>() {
                let s = sample? as f64;
                let amplified = (s * (gain as f64)).clamp(-max_val as f64, max_val as f64);
                writer.write_sample(amplified as i32)?;
            }
        }
        hound::SampleFormat::Float => {
            for sample in reader.samples::<f32>() {
                let s: f32 = sample?;
                let amplified: f32 = (s * gain).clamp(-1.0_f32, 1.0_f32);
                writer.write_sample(amplified)?;
            }
        }
    }

    writer.finalize()?;
    println!("✅ Gain applied successfully! Output written to '{}'", output_path);
    Ok(())
}
