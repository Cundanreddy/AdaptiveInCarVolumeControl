# Adaptive In-Car Volume — STM32F407 (Rust)


## Build


1. Install Rust + target:


```bash
rustup default stable
rustup target add thumbv7em-none-eabihf
cargo install cargo-embed
```


2. Build (release recommended):


```bash
cargo build --release
```


3. Flash & debug (using cargo-embed / probe-rs):


```bash
cargo embed --release
```


If you prefer `openocd` or `st-flash`, produce an ELF with `cargo build` and use your usual toolchain.


## Notes
- The `src/main.rs` included is scaffold: adapt DMA / I2S examples from `stm32f4xx-hal` and the `rtic` examples for correct APIs.
- Use small ADC buffer sizes while you iterate (e.g., 256 samples) to reduce latency.
- The example intentionally separates concerns: `adc -> rms -> smoother -> gain -> i2s`.
