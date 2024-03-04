use clap::Parser;
use color_eyre::eyre::{self as anyhow, WrapErr};
use red_button_trigger_timestamp_comms::{FromDevice, ToDevice};
use tokio::io::AsyncWriteExt;
use tokio_serial::SerialPortBuilderExt;
use json_lines::codec::JsonLinesCodec;
use futures::{SinkExt, StreamExt};
use tokio::sync::watch;
use tokio_serial::SerialStream;

#[derive(Parser)]
struct Cli {
    /// Serial device to open
    device_path: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // construct a subscriber that prints formatted traces to stdout
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber)?;

    let opt = Cli::parse();
    let device_path = opt.device_path;
    let baud_rate = 115_200;
    tracing::info!("Opening device at path {}", device_path);

    #[allow(unused_mut)]
    let mut serial_device = tokio_serial::new(&device_path, baud_rate)
        .open_native_async()
        .with_context(|| format!("opening device {device_path}"))?;

    #[cfg(unix)]
    serial_device.set_exclusive(false)
        .expect("Unable to set serial port exclusive to false");

    let framed = tokio_util::codec::Framed::new(serial_device, JsonLinesCodec::<FromDevice,ToDevice>::default());

    let (mut device_tx, mut device_rx) = framed.split();

    let resp = device_tx.send(ToDevice::Ping(1234)).await?;

    loop {
        while let Some(resp) = device_rx.next().await {
            let resp = resp?;
            tracing::info!("response: {:?}",resp);
        }
    }
    Ok(())
}
