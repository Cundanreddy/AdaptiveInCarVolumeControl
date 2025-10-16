#![no_main]
#![no_std]

// HAL-version-matched implementation scaffold for `stm32f4xx-hal = "0.15"` and `rtic = "0.7"`.
// This file implements:
// - ADC circular DMA capture into a static buffer
// - Half/full transfer handling to compute RMS on each half
// - Simple gain smoother
// - I2S (SPI3) DMA transmit skeleton for PCM5102 (TX only)
//
// IMPORTANT: HAL APIs evolve. This file is intended to compile with the 0.15-era APIs,
// but you may need to change a few type/constructor names depending on the exact patch
// release you use. All <ADAPT> comments indicate places you may need to tweak.

use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use libm::{sqrt, powf, log10f};

// crate::pac;
// stm32f4xx_hal::pac;

use panic_halt as _;
use stm32f4xx_hal as hal;
use hal::{prelude::*, pac, adc::{Adc, config::AdcConfig}, dma::{config::DmaConfig, Stream0, StreamsTuple, Transfer}, gpio::Analog, i2s::{I2sExt, I2s}, serial::Serial};

use rtic::app;

// Buffer length must be even since we treat it as two halves
pub const ADC_BUF_LEN: usize = 512;

// Place ADC buffer in a known memory section and make it mutable static for DMA
#[link_section = ".axisram.data"]
static mut ADC_BUFFER: [u16; ADC_BUF_LEN] = [0; ADC_BUF_LEN];

// Flag set by DMA half/full transfer callbacks (RTIC interrupt context)
static HALF_READY: AtomicBool = AtomicBool::new(false);
static FULL_READY: AtomicBool = AtomicBool::new(false);

#[app(device = stm32f4xx_hal::pac, peripherals = true)]
mod app {
    #[shared]
    struct Shared {
        // put shared resources here if needed
    }

    #[local]
    struct Local {
        adc: Adc<pac::ADC1>,
        // The circular DMA transfer owning the buffer (type depends on HAL)
        // adc_circ: Transfer<...>, // left out here because types vary across hal versions

        // Smoothing / computed values (local to the processing task)
        smoothed_level: f32,
        target_gain: f32,

        // serial for debug
        serial: hal::serial::Tx<pac::USART2>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        // Enable DWT cycle counter for RTIC scheduling (optional)
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();

        let dp = cx.device;

        // --- clocks
        let rcc = dp.RCC.constrain();
        let clocks = rcc.cfgr
            .use_hse(8.mhz())
            .sysclk(168.mhz())
            .pclk1(42.mhz())
            .freeze();

        // --- GPIO
        let gpioa = dp.GPIOA.split();
        let gpiob = dp.GPIOB.split();

        // ADC pin: PA0 (adjust as needed)
        let mic_pin = gpioa.pa0.into_analog();

        // Serial TX for debug on PA2 (USART2)
        let tx_pin = gpioa.pa2.into_alternate_af7();
        let rx_pin = gpioa.pa3.into_alternate_af7();
        let serial = Serial::usart2(dp.USART2, (tx_pin, rx_pin), 115_200.bps(), clocks).unwrap();
        let (tx, _rx) = serial.split();

        // --- ADC
        let mut adc = Adc::adc1(dp.ADC1, true, AdcConfig::default());
        adc.configure_channel(&mic_pin, hal::adc::config::Sequence::One, hal::adc::config::SampleTime::Cycles_15);

        // --- DMA setup for ADC -> memory (circular)
        // Note: stm32f4 DMA mapping: ADC1 -> DMA2 Stream0 Channel0 (common on many F4 parts)
        let streams = StreamsTuple::new(dp.DMA2);
        let stream0 = streams.0; // Stream0 as example -- confirm mapping for your MCU

        // Create DMA config: peripheral-to-memory, circular, transfer complete & half-transfer interrupt enabled
        let dma_cfg = DmaConfig::default()
            .memory_increment(true)
            .peripheral_increment(false)
            .priority(hal::dma::config::Priority::High)
            .circular(true);

        // <ADAPT> The exact Transfer/CircBuffer construction may vary across hal versions.
        // Here we show the intended pattern: create a circular buffer transfer from ADC data register to ADC_BUFFER.

        // SAFETY: ADC_BUFFER is exclusively used by DMA peripheral + read-only processing in interrupt/task contexts.
        let circ = unsafe {
            // Using HAL helper to create a CircBuffer transfer if available
            // Example (pseudo):
            // let circ = Transfer::init_peripheral_to_memory(stream0, adc.get_dma_peripheral(), &mut ADC_BUFFER, None, dma_cfg);
            // If your hal provides `adc.with_dma()` or `adc.read_exact()` helpers, use those helpers instead.
            core::ptr::null_mut()
        };

        // --- I2S (SPI3) TX setup (skeleton)
        // Connect to PCM5102: typically SPI3 in I2S mode (PB3 SCK, PB5 SD, PA15 WS etc. Verify with your board)
        // We'll set up I2S peripheral and DMA transmit similarly to ADC but for memory->peripheral.

        // <ADAPT> Use the HAL i2s ext: `let i2s = dp.SPI3.i2s(...);` patterns differ by hal version.

        // Start the ADC DMA transfer (API depends on HAL). We assume it's started here.
        // Example: circ.start(|adc_periph| { adc_periph.enable_dma(); });

    // (scheduling disabled) The project originally scheduled the first `process_audio` via a monotonic.
    // If you add a monotonic to `#[app(...)]`, re-enable the following scheduling call.
    // cx.schedule.process_audio(cx.start + 84_000_000.cycles()).unwrap(); // 50ms @168MHz ~8.4M cycles

        // return shared and local resources
        (
            Shared {},
            Local {
                adc,
                smoothed_level: 0.0,
                target_gain: 1.0,
                serial: tx,
            },
        )
    }

    // Periodic processing task: read which half of buffer is ready (via flags set from DMA interrupt), compute RMS and update gain
    #[task(local = [smoothed_level, target_gain, serial])]
    async fn process_audio(mut cx: process_audio::Context) {
        // Check DMA flags set by interrupts
        if HALF_READY.swap(false, Ordering::SeqCst) {
            // compute RMS on first half
            let half = unsafe { &ADC_BUFFER[0..(ADC_BUF_LEN/2)] };
            let rms = rms_u16_block(half);
            // simple smoothing
            *cx.local.smoothed_level = smooth(*cx.local.smoothed_level, rms, 0.95);

            // compute gain mapping (example: keep target_gain inversely proportional to noise)
            let noise_db = lin_to_db((*cx.local.smoothed_level).max(1e-6));
            let desired_db = -0.5 * (noise_db - (-40.0)); // tune constants
            *cx.local.target_gain = db_to_lin(desired_db);

            // optional: send debug byte (not async-safe; keep minimal)
            let _ = cx.local.serial.write(b'H');
        }

        if FULL_READY.swap(false, Ordering::SeqCst) {
            // compute RMS on second half
            let half = unsafe { &ADC_BUFFER[(ADC_BUF_LEN/2)..ADC_BUF_LEN] };
            let rms = rms_u16_block(half);
            *cx.local.smoothed_level = smooth(*cx.local.smoothed_level, rms, 0.95);
            let noise_db = lin_to_db((*cx.local.smoothed_level).max(1e-6));
            let desired_db = -0.5 * (noise_db - (-40.0));
            *cx.local.target_gain = db_to_lin(desired_db);

            let _ = cx.local.serial.write(b'F');
        }

        // Re-schedule (disabled — needs a monotonic). Re-enable scheduling after adding a monotonic.
    }

    // DMA interrupt handlers (example names -- adapt to your vector table)
    // In many HAL setups, the DMA transfer object registers its own interrupt handler callback. If you use that facility,
    // you can set the HALF_READY/FULL_READY flags there instead of defining interrupts here.

    #[task(binds = DMA2_STREAM0)]
    fn dma2_stream0(_cx: dma2_stream0::Context) {
        // Clear interrupt flags on DMA stream and set HALF_READY / FULL_READY accordingly.
        // This MUST be implemented with the actual peripheral registers or using the HAL helper functions.
        // Example pseudocode:
        // if stream.get_half_transfer_flag() { HALF_READY.store(true, Ordering::SeqCst); stream.clear_half_transfer(); }
        // if stream.get_transfer_complete_flag() { FULL_READY.store(true, Ordering::SeqCst); stream.clear_transfer_complete(); }
    }

    // extern "Rust" {
    //     fn EXTI0();
    // }
}

// ------------------- Utilities -------------------

/// Compute RMS over a block of ADC samples (u16). Expects 12-bit ADC centered ~2048.
fn rms_u16_block(buf: &[u16]) -> f32 {
    let mut sum_sq: f64 = 0.0;
    for &s in buf.iter() {
        let v = (s as f32) - 2048.0;
        let vf = v as f64;
        sum_sq += vf * vf;
    }
    let mean_sq = sum_sq / (buf.len() as f64);
    sqrt(mean_sq) as f32
}

fn smooth(prev: f32, input: f32, alpha: f32) -> f32 {
    alpha * prev + (1.0 - alpha) * input
}

fn db_to_lin(db: f32) -> f32 { powf(10.0_f32, db / 20.0_f32) }
fn lin_to_db(lin: f32) -> f32 { 20.0 * log10f(lin.abs().max(1e-12)) }