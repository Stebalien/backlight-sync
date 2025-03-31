use std::ffi::OsStr;
use std::future::IntoFuture;
use std::io;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ddc_hi::{Ddc, Display};
use futures::stream::StreamExt;
use tokio::task::JoinError;
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

async fn update_brightness(displays: Vec<Arc<Mutex<Display>>>, brightness: u16) {
    time::sleep(UPDATE_DELAY).await;
    let mut js = tokio::task::JoinSet::new();
    for display in displays {
        js.spawn_blocking(move || {
            let mut display = display.lock().unwrap();
            for _ in 0..3 {
                if let Err(e) = display.handle.set_vcp_feature(0x10, brightness) {
                    log::warn!(
                        "failed to set brightness for display {}: {}",
                        display.info,
                        e
                    );
                } else {
                    break;
                }
            }
        });
    }
    let _ = js.join_all().await;
}

async fn enumerate() -> Result<Vec<Arc<Mutex<Display>>>, JoinError> {
    tokio::task::spawn_blocking(|| {
        Display::enumerate()
            .into_iter()
            .map(|d| Arc::new(Mutex::new(d)))
            .collect()
    })
    .into_future()
    .await
}

#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut brightness: u16 = get_initial_brightness()?.unwrap_or(255);
    let mut displays = enumerate().await?;

    let mut monitor = AsyncMonitorSocket::try_from(
        MonitorBuilder::new()?
            .match_subsystem("backlight")?
            .match_subsystem("drm")?
            .listen()?,
    )?;

    let mut update_task = Some(update_brightness(displays.clone(), brightness));
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
                displays = enumerate().await?;
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
        update_task = Some(update_brightness(displays.clone(), brightness));
    }
}
