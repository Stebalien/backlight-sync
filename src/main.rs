use std::ffi::OsStr;
use std::future::IntoFuture;
use std::io;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ddc_hi::{Ddc, Display};
use futures::stream::StreamExt;
use mccs_db::Access;
use tokio::task::JoinError;
use tokio_udev::{AsyncMonitorSocket, Device, Enumerator, MonitorBuilder};

const MONITOR_CHANGE_DELAY: Duration = Duration::from_secs(5);
const VCP_SET_BRIGHTNESS: u8 = 0x10;

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

async fn update_brightness(displays: &[Arc<Mutex<Display>>], brightness: u16) -> bool {
    let mut js = tokio::task::JoinSet::new();
    #[allow(clippy::unnecessary_to_owned)] // clippy is drunk here.
    for display in displays.iter().cloned() {
        js.spawn_blocking(move || {
            let mut display = display.lock().unwrap();
            for _ in 0..3 {
                match display.handle.set_vcp_feature(0x10, brightness) {
                    Ok(_) => return true,
                    Err(e) => log::warn!(
                        "failed to set brightness for display {}: {}",
                        display.info,
                        e
                    ),
                }
            }
            false
        });
    }
    js.join_all().await.into_iter().all(|b| b)
}

async fn enumerate() -> Result<Vec<Arc<Mutex<Display>>>, JoinError> {
    tokio::task::spawn_blocking(|| {
        let displays = Display::enumerate();
        let mut output = Vec::with_capacity(displays.len());
        for mut display in displays {
            if let Err(e) = display.update_capabilities() {
                log::error!(
                    "failed to update display capabilities for {}, skipping display: {}",
                    display.info,
                    e
                );
                continue;
            }
            match display.info.mccs_database.get(VCP_SET_BRIGHTNESS) {
                Some(v) => match v.access {
                    Access::WriteOnly | Access::ReadWrite => {}
                    Access::ReadOnly => {
                        log::debug!(
                            "skipping display {}, cannot update brightness",
                            display.info
                        );
                        continue;
                    }
                },
                None => continue,
            }
            output.push(Arc::new(Mutex::new(display)));
        }
        output
    })
    .into_future()
    .await
}

#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut displays = enumerate().await?;
    let mut monitor = AsyncMonitorSocket::try_from(
        MonitorBuilder::new()?
            .match_subsystem("backlight")?
            .match_subsystem("drm")?
            .listen()?,
    )?;

    let mut brightness: u16 = get_initial_brightness()?.unwrap_or(255);
    update_brightness(&displays, brightness).await;
    while let Some(event) = monitor.next().await {
        let event = event?;
        match event.device().subsystem().and_then(OsStr::to_str) {
            Some("drm") => {
                // refresh
                log::info!("drm change, updating backlight");
                // I wait a second here to avoid...
                tokio::time::sleep(MONITOR_CHANGE_DELAY).await;
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
        if !update_brightness(&displays, brightness).await {
            log::warn!("failed to set brightness for some displays, refreshing display list");
            displays = enumerate().await?;
        }
    }
    Ok(())
}
