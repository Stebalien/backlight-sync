// A daemon that synchronizes external display brightness with the system's built-in
// display brightness.
//
// Uses DDC/CI protocol over I2C to control external monitors when the internal
// backlight brightness changes.

use std::ffi::OsStr;
use std::future::IntoFuture;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use ddc::Ddc;
use ddc_i2c::{from_i2c_device, I2cDeviceDdc};
use futures::stream::StreamExt;
use mccs_db::Access;
use tokio_udev::{AsyncMonitorSocket, Device, Enumerator, MonitorBuilder};

// How long to wait after display connection changes before refreshing
const MONITOR_CHANGE_DELAY: Duration = Duration::from_secs(5);
// MCCS Virtual Control Panel code for brightness
const VCP_SET_BRIGHTNESS: u8 = 0x10;

/// Extracts a typed attribute from a udev device
fn get_attribute<T: FromStr>(dev: &Device, attr: &str) -> Option<T> {
    dev.attribute_value(attr)
        .and_then(OsStr::to_str)
        .and_then(|s| s.parse().ok())
}

/// Gets normalized brightness (0-65535) from a backlight device
fn get_brightness(dev: &Device) -> Option<u16> {
    let brightness: u64 = get_attribute(dev, "brightness")?;
    let max_brightness: u64 = get_attribute(dev, "max_brightness")?;
    Some(
        (brightness.saturating_mul(u16::MAX.into()) / max_brightness).clamp(0, u16::MAX.into())
            as u16,
    )
}

/// Retrieves the initial brightness setting from the internal display
fn get_initial_brightness() -> anyhow::Result<Option<u16>> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_is_initialized()?;
    enumerator.match_subsystem("backlight")?;
    Ok(enumerator
        .scan_devices()?
        .filter_map(|d| get_brightness(&d))
        .next())
}

/// Sets brightness on all connected external displays
/// Returns true if all displays were updated successfully
async fn update_brightness(displays: &[Arc<Mutex<I2cDeviceDdc>>], brightness: u16) -> bool {
    let mut js = tokio::task::JoinSet::new();
    // Convert from 16-bit range to 0-100 scale used by MCCS
    let brightness = (((brightness as u32) * 100) / u16::MAX as u32) as u16;
    #[allow(clippy::unnecessary_to_owned)] // clippy is drunk here.
    for display in displays.iter().cloned() {
        js.spawn_blocking(move || {
            let mut display = display.lock().unwrap();
            // Retry up to 3 times, as DDC can be flaky
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

/// Determines if a device has a backlight (likely the internal display)
fn has_backlight(d: &Device) -> anyhow::Result<bool> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_is_initialized()?;
    enumerator.match_parent(d)?;
    enumerator.match_subsystem("backlight")?;
    Ok(enumerator.scan_devices()?.next().is_some())
}

/// Creates an enumerator for finding connected and enabled displays
fn connected_displays() -> anyhow::Result<Enumerator> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_is_initialized()?;
    enumerator.match_subsystem("drm")?;
    enumerator.match_attribute("status", "connected")?;
    enumerator.match_attribute("enabled", "enabled")?;
    Ok(enumerator)
}

/// Finds the I2C device associated with a display device
fn i2c_device(parent: &Device) -> anyhow::Result<Option<I2cDeviceDdc>> {
    let mut enumerator = Enumerator::new()?;
    enumerator.match_is_initialized()?;
    enumerator.match_parent(parent)?;
    enumerator.match_subsystem("i2c-dev")?;
    for d in enumerator.scan_devices()? {
        if let Some(dev) = d.property_value("DEVNAME") {
            return Ok(Some(from_i2c_device(dev)?));
        };
    }
    Ok(None)
}

/// Retrieves MCCS capabilities from an I2C device
fn get_capabilities(i2c_device: &mut I2cDeviceDdc) -> anyhow::Result<mccs_db::Database> {
    let caps = i2c_device
        .capabilities_string()
        .context("failed to read capabilities")?;
    let caps = mccs_caps::parse_capabilities(caps).context("failed to parse capabilities")?;

    let mut db = caps
        .mccs_version
        .as_ref()
        .map(mccs_db::Database::from_version)
        .unwrap_or_default();
    db.apply_capabilities(&caps);
    Ok(db)
}

/// Discovers compatible external displays that support brightness control
async fn enumerate() -> anyhow::Result<Vec<Arc<Mutex<I2cDeviceDdc>>>> {
    tokio::task::spawn_blocking(|| {
        let mut output = Vec::new();
        for d in connected_displays()?.scan_devices()? {
            // Skip the primary backlight device (internal display)
            if has_backlight(&d)? {
                continue;
            }

            // Get the associated i2c device, if any
            let Some(mut i2cdev) = i2c_device(&d)? else {
                continue;
            };

            let sysname = d.sysname();
            let caps = match get_capabilities(&mut i2cdev) {
                Ok(caps) => caps,
                Err(e) => {
                    log::warn!("failed to get capabilities for {sysname:?}: {e}");
                    continue;
                }
            };

            // Only include displays that support brightness control
            match caps.get(VCP_SET_BRIGHTNESS) {
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
        Ok(output)
    })
    .into_future()
    .await
    .unwrap_or_else(|e| Err(anyhow!(e)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Find compatible external displays
    let mut displays = enumerate().await?;

    // Monitor for backlight and display connection changes
    let mut monitor = AsyncMonitorSocket::try_from(
        MonitorBuilder::new()?
            .match_subsystem("backlight")?
            .match_subsystem("drm")?
            .listen()?,
    )?;

    // Get initial brightness and apply to all displays
    let mut brightness: u16 = get_initial_brightness()?.unwrap_or(u16::MAX);
    update_brightness(&displays, brightness).await;

    // Main event loop
    while let Some(event) = monitor.next().await {
        let event = event?;
        match event.device().subsystem().and_then(OsStr::to_str) {
            Some("drm") => {
                // Display connection state changed
                log::info!("drm change, updating backlight");
                // Wait to avoid race conditions during connection
                tokio::time::sleep(MONITOR_CHANGE_DELAY).await;
                displays = enumerate().await?;
            }
            Some("backlight") => {
                // Internal display brightness changed
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
        // Apply brightness change to external displays, refresh list if needed
        if !update_brightness(&displays, brightness).await {
            log::warn!("failed to set brightness for some displays, refreshing display list");
            displays = enumerate().await?;
        }
    }
    Ok(())
}
