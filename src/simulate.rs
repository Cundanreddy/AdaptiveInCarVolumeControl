mod gain;
mod audio;

fn main() -> anyhow::Result<()> {
    println!("🎧 Adaptive In-Car Volume Normalization (Rust)");
    audio::run_audio_loop()
}
