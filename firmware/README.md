# red-button-trigger-timestamp-firmware 🔘 for Raspberry Pi Pico

## Building

### Install prerequisites

```
rustup target install thumbv6m-none-eabi
cargo install flip-link --locked
cargo install elf2uf2-rs --locked
```

### Build firmware

```
cargo build --release
elf2uf2-rs target/thumbv6m-none-eabi/release/red-button-trigger-timestamp-firmware
```

You should now have the file `red-button-trigger-timestamp-firmware.uf2` in the
`target/thumbv6m-none-eabi/release` directory. This is the firmware file which
you should flash to your pico.

### Install firmware

Hold down the BOOTSEL (short for boot select) button on the Pico and plug it
into your machine. It should appear as a flash drive. Copy the
`red-button-trigger-timestamp-firmware.uf2` file to this "flash drive".

## Debugging with Knurling (`probe-rs`)

We use the Knurling project to facilitate debugging. `probe-rs` can be used to
debug the device from a host computer and view log messages send using the
`defmt` infrastructure. Install `probe-run` with `cargo install probe-run --locked`.

To see `defmt` messages, compile with the `DEFMT_LOG` environment variable
set appropriately. (By default, `defmt` will show only error level messages.)

Powershell (Windows)
```
$Env:DEFMT_LOG="trace"
```

Bash (Linux/macOS)
```
export DEFMT_LOG=trace
```

### Probe

Run with:

```
cargo run --release
```

# License

Portions of this project are derived from the cortex-m-quickstart project, which
is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
