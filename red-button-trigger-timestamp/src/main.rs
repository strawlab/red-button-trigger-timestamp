use clap::Parser;
use color_eyre::eyre::{self as anyhow, WrapErr};
use futures::{SinkExt, StreamExt};
use json_lines::codec::JsonLinesCodec;
use red_button_trigger_timestamp_comms::{FromDevice, ToDevice, VersionResponse};
use serde::Serialize;
use tokio_serial::SerialPortBuilderExt;
use tracing_subscriber::{fmt, layer::SubscriberExt};

mod clock_model;

#[derive(Serialize)]
struct TriggerRow {
    timestamp_local: chrono::DateTime<chrono::Local>,
    epoch_nanos_utc: i64,
}

#[derive(Parser)]
struct Cli {
    /// Serial device to open
    device_path: Option<String>,

    /// Output directory
    #[arg(short, long, default_value = "~/TRIGGER_DATA")]
    output_dir: String,
}

fn to_device_name(spi: &tokio_serial::SerialPortInfo) -> String {
    let name = spi.port_name.clone();
    // This is necessary on linux:
    name.replace("/sys/class/tty/", "/dev/")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    let collector = tracing_subscriber::registry()
        .with(fmt::layer())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env());
    tracing::subscriber::set_global_default(collector)?;

    let opt = Cli::parse();
    let device_path = match opt.device_path {
        None => {
            let available_ports: Vec<_> = tokio_serial::available_ports()?
                .iter()
                .map(to_device_name)
                .filter(|x| x != "/dev/ttyS0")
                .collect();
            println!("No device path was given. Available options:");
            for p in available_ports.iter() {
                println!("{p}");
            }
            return Ok(());
        }
        Some(p) => p,
    };
    let baud_rate = 115_200;
    tracing::info!("Opening device at path {}", device_path);

    #[allow(unused_mut)]
    let mut serial_device = tokio_serial::new(&device_path, baud_rate)
        .open_native_async()
        .with_context(|| format!("opening device {device_path}"))?;
    tracing::info!("Device opened");

    #[cfg(unix)]
    serial_device
        .set_exclusive(false)
        .expect("Unable to set serial port exclusive to false");

    let local = chrono::Local::now();
    let output_filename_template = "triggers_%Y%m%d_%H%M%S.csv".to_string();
    let filename = local.format(&output_filename_template).to_string();

    let output_dir = std::path::PathBuf::from(shellexpand::full(&opt.output_dir)?.to_string());
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("ensuring existence of directory {}", output_dir.display()))?;

    let full_path = output_dir.join(filename);
    let fd = std::fs::File::create(&full_path)
        .with_context(|| format!("creating file {}", full_path.display()))?;
    tracing::info!("Saving data to {}", full_path.display());
    let mut csv_wtr = csv::Writer::from_writer(fd);

    let framed = tokio_util::codec::Framed::new(
        serial_device,
        JsonLinesCodec::<FromDevice, ToDevice>::default(),
    );

    let (mut device_tx, mut device_rx) = framed.split();

    device_tx.send(ToDevice::VersionRequest).await?;
    let version_request_sent = std::time::Instant::now();
    let mut did_receive_version_response = false;

    let mut last_ping = chrono::Utc::now();
    let mut last_pong = chrono::Utc::now();

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut clock_model = clock_model::ClockModel::default();
    loop {
        tokio::select! {
            Some(from_device) = device_rx.next() => {
                let recv_time = chrono::Utc::now();
                match from_device? {
                    FromDevice::Pong(device_timestamp) => {
                        last_pong = chrono::Utc::now();
                        clock_model.update(last_ping,recv_time,device_timestamp);
                        tracing::debug!("pong utc: {:?}", clock_model.compute_utc(device_timestamp));
                    }
                    FromDevice::Trigger(device_timestamp) => {
                        if let Some(trigger_utc) = clock_model.compute_utc(device_timestamp) {
                            let timestamp_local: chrono::DateTime<chrono::Local> =
                            trigger_utc.with_timezone(&chrono::Local);
                            tracing::info!("trigger: {}", timestamp_local);
                            let delta_epoch = trigger_utc - chrono::DateTime::UNIX_EPOCH;
                            let epoch_nanos_utc = delta_epoch.num_nanoseconds().unwrap();
                            let trig_row = TriggerRow {
                                timestamp_local,
                                epoch_nanos_utc,
                            };
                            csv_wtr.serialize(trig_row)?;
                            csv_wtr.flush()?;
                        } else {
                            tracing::error!("Could not compute trigger time.");
                        }
                    }
                    FromDevice::VersionResponse(info) => {
                        let my_info = VersionResponse::default();
                        if info != my_info {
                            anyhow::bail!("firmware has version {:?}, but program has version {:?}", info,my_info);
                        }
                        tracing::info!("Connected to firmware \"{}\" v{}", String::from_utf8_lossy(&info.name), info.version);
                        did_receive_version_response = true;
                    }
                }
            }
            _ = interval.tick() => {
                last_ping = chrono::Utc::now();
                device_tx.send(ToDevice::Ping).await?;
                let delta = last_ping - last_pong;
                if delta > chrono::TimeDelta::seconds(5) {
                    tracing::error!("No communication with device in {} seconds.", delta.num_milliseconds()as f64/1000.0);
                }
            }
        }

        if !did_receive_version_response
            && version_request_sent.elapsed() > std::time::Duration::from_secs(5)
        {
            anyhow::bail!("No version response received.");
        }
    }
}
