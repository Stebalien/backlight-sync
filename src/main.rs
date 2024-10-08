use std::ffi::OsStr;
use std::io;
use std::str::FromStr;
use std::time::Duration;

use ddc_hi::{Ddc, Display};
use futures::stream::StreamExt;
use tokio::time;
use tokio_udev::{AsyncMonitorSocket, Device, Enumerator, MonitorBuilder};

const UPDATE_DELAY: Duration = Duration::from_secs(1);

fn get_attribute<T: FromStr>(dev: &Device, attr: &str) -> Option<T> {
    dev.attribute_value(attr)
        .and_then(OsStr::to_str)
        .and_then(|s| s.parse().ok())
}

fn get_brightness(dev: &Device) -> Option<u16> {
    let brightness: u16 = get_attribute(dev, "brightness")?;
    let max_brightness: u16 = get_attribute(dev, "max_brightness")?;
    Some(brightness * 100 / max_brightness)
}

fn get_initial_brightness() -> io::Result<Option<u16>> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_is_initialized()?;
    enumerator.match_subsystem("backlight")?;
    Ok(enumerator
        .scan_devices()?
        .filter_map(|d| get_brightness(&d))
        .next())
}

async fn update_brightness(brightness: u16) {
    'retry: for _ in 0..3 {
        time::sleep(UPDATE_DELAY).await;
        for mut display in Display::enumerate() {
            if let Err(e) = display.handle.set_vcp_feature(0x10, brightness) {
                log::warn!(
                    "failed to set brightness for display {}: {}",
                    display.info,
                    e
                );
                continue 'retry;
            };
        }
        break;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    env_logger::init();
    let mut brightness: u16 = get_initial_brightness()?.unwrap_or(255);
    let mut monitor = AsyncMonitorSocket::try_from(
        MonitorBuilder::new()?
            .match_subsystem("backlight")?
            .match_subsystem("drm")?
            .listen()?,
    )?;

    let mut update_task = Some(update_brightness(brightness));
    loop {
        let Some(event) = if let Some(task) = update_task.take() {
            tokio::select! {
                event = monitor.next() => event,
                _ = task => {
                    monitor.next().await
                }
            }
        } else {
            monitor.next().await
        }
        .transpose()?
        else {
            return Ok(());
        };

        match event.device().subsystem().and_then(OsStr::to_str) {
            Some("drm") => {
                // refresh
                log::info!("drm change, updating backlight");
            }
            Some("backlight") => {
                log::debug!("got backlight change event: {event:?}");
                let Some(new_brightness) = get_brightness(&event) else {
                    continue;
                };
                if new_brightness == brightness {
                    continue;
                }
                log::info!("changing backlight from {brightness} to {new_brightness}");
                brightness = new_brightness;
            }
            _ => continue,
        }
        update_task = Some(update_brightness(brightness));
    }
}
