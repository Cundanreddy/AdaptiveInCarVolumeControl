use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use crate::gain::AdaptiveGain;
use std::sync::{Arc, Mutex};

pub fn run_audio_loop() -> anyhow::Result<()> {
    let host = cpal::default_host();

    let input_device = host.default_input_device().expect("no input device available");
    let output_device = host.default_output_device().expect("no output device available");

    println!("Input: {:?}", input_device.name()?);
    println!("Output: {:?}", output_device.name()?);

    let config = input_device.default_input_config()?;
    let sample_rate = config.sample_rate().0 as f32;

    let shared_gain = Arc::new(Mutex::new(AdaptiveGain::new(75.0, 0.1, 1.0, 0.0)));
    let output_gain = shared_gain.clone();

    // Simulate speed (sine)
    let mut speed = 0.0f32;

    let input_stream = match config.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&input_device, &output_device, sample_rate, shared_gain, &mut speed)?,
        SampleFormat::I16 => build_stream::<i16>(&input_device, &output_device, sample_rate, shared_gain, &mut speed)?,
        SampleFormat::U16 => build_stream::<u16>(&input_device, &output_device, sample_rate, shared_gain, &mut speed)?,
    };

    input_stream.play()?;
    std::thread::park(); // keep alive
    Ok(())
}

fn build_stream<T>(
    input_device: &cpal::Device,
    output_device: &cpal::Device,
    sample_rate: f32,
    gain_ref: Arc<Mutex<AdaptiveGain>>,
    speed_ref: &mut f32,
) -> anyhow::Result<cpal::Stream>
where
    T: Sample + cpal::FromSample<f32> + cpal::SizedSample,
{
    let config = input_device.default_input_config()?.config();
    let channels = config.channels as usize;

    let mut output_stream = output_device.build_output_stream_raw(
        &config,
        SampleFormat::F32,
        move |data, _: &cpal::OutputCallbackInfo| {
            let buffer = data.as_slice::<f32>().unwrap();
            for s in buffer.iter_mut() {
                *s = 0.0;
            }
        },
        move |err| eprintln!("output err: {err:?}"),
    )?;
    output_stream.play()?;

    let mut frame_count = 0u64;

    let stream = input_device.build_input_stream(
        &config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut rms = 0.0f32;
            for sample in data.iter().step_by(channels) {
                let v = sample.to_f32();
                rms += v * v;
            }
            rms = (rms / (data.len() as f32 / channels as f32)).sqrt();
            let cabin_db = 20.0 * rms.max(1e-6).log10() + 94.0;

            *speed_ref = 60.0 + 20.0 * ((frame_count as f32 / sample_rate) * 0.05).sin();
            frame_count += data.len() as u64 / channels as u64;

            let mut gain = gain_ref.lock().unwrap();
            let (gain_db, gain_lin) = gain.compute_gain(cabin_db, *speed_ref);

            println!("Cabin: {:.1} dB | Speed: {:.1} | Gain: {:.2} dB", cabin_db, *speed_ref, gain_db);

            // Normally apply gain to playback buffer here (loopback / file)
            // For demo, we just print gain values.

        },
        move |err| eprintln!("input err: {err:?}"),
        None,
    )?;
    Ok(stream)
}
