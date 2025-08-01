use std::ffi::OsStr;
use std::future::IntoFuture;
use std::io;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ddc::Ddc;
use ddc_i2c::{from_i2c_device, I2cDeviceDdc};
use futures::stream::StreamExt;
use mccs_db::Access;
use tokio_udev::{AsyncMonitorSocket, Device, Enumerator, MonitorBuilder};

const MONITOR_CHANGE_DELAY: Duration = Duration::from_secs(5);
const VCP_SET_BRIGHTNESS: u8 = 0x10;

fn get_attribute<T: FromStr>(dev: &Device, attr: &str) -> Option<T> {
    dev.attribute_value(attr)
        .and_then(OsStr::to_str)
        .and_then(|s| s.parse().ok())
}

fn get_brightness(dev: &Device) -> Option<u16> {
    let brightness: u64 = get_attribute(dev, "brightness")?;
    let max_brightness: u64 = get_attribute(dev, "max_brightness")?;
    Some(
        (brightness.saturating_mul(u16::MAX.into()) / max_brightness).clamp(0, u16::MAX.into())
            as u16,
    )
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

async fn update_brightness(displays: &[Arc<Mutex<I2cDeviceDdc>>], brightness: u16) -> bool {
    let mut js = tokio::task::JoinSet::new();
    let brightness = (((brightness as u32) * 100) / u16::MAX as u32) as u16;
    #[allow(clippy::unnecessary_to_owned)] // clippy is drunk here.
    for display in displays.iter().cloned() {
        js.spawn_blocking(move || {
            let mut display = display.lock().unwrap();
            for _ in 0..3 {
                match display.set_vcp_feature(0x10, brightness) {
                    Ok(_) => return true,
                    Err(e) => log::warn!("failed to set brightness for display: {e}"),
                }
            }
            false
        });
    }
    js.join_all().await.into_iter().all(|b| b)
}

async fn enumerate() -> io::Result<Vec<Arc<Mutex<I2cDeviceDdc>>>> {
    tokio::task::spawn_blocking(|| {
        let mut output = Vec::new();
        let mut enumerator = Enumerator::new()?;
        enumerator.match_is_initialized()?;
        enumerator.match_subsystem("drm")?;
        enumerator.match_attribute("status", "connected")?;
        enumerator.match_attribute("enabled", "enabled")?;
        for d in enumerator.scan_devices()? {
            let sysname = d.sysname();
            let mut ddc_enum = Enumerator::new()?;
            ddc_enum.match_parent(&d)?;
            ddc_enum.match_subsystem("i2c-dev")?;
            for d in ddc_enum.scan_devices()? {
                let Some(dev) = d.property_value("DEVNAME") else {
                    continue;
                };
                let mut i2cdev = from_i2c_device(dev)?;
                let caps = match i2cdev.capabilities_string() {
                    Ok(caps) => caps,
                    Err(e) => {
                        log::warn!("failed to read {sysname:?} capabilities: {e}");
                        continue;
                    }
                };
                let caps = match mccs_caps::parse_capabilities(caps) {
                    Ok(caps) => caps,
                    Err(e) => {
                        log::warn!("failed to parse {sysname:?} capabilities: {e}");
                        continue;
                    }
                };
                let Some(mccs_version) = caps.mccs_version else {
                    continue;
                };

                let mut db = mccs_db::Database::from_version(&mccs_version);
                db.apply_capabilities(&caps);

                match db.get(VCP_SET_BRIGHTNESS) {
                    Some(v) => match v.access {
                        Access::WriteOnly | Access::ReadWrite => {}
                        Access::ReadOnly => {
                            log::debug!("skipping display {sysname:?}, cannot update brightness");
                            continue;
                        }
                    },
                    None => continue,
                }
                output.push(Arc::new(Mutex::new(i2cdev)));
            }
        }
        Ok(output)
    })
    .into_future()
    .await
    .unwrap_or_else(|e| Err(io::Error::other(e)))
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

    let mut brightness: u16 = get_initial_brightness()?.unwrap_or(u16::MAX);
    update_brightness(&displays, brightness).await;
    while let Some(event) = monitor.next().await {
        let event = event?;
        match event.device().subsystem().and_then(OsStr::to_str) {
            Some("drm") => {
                // refresh
                log::info!("drm change, updating backlight");
                // I wait a second here to avoid some race conditions when connecting.
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
