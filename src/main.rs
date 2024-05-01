use std::ffi::OsStr;
use std::io;
use std::str::FromStr;

use ddc_hi::{Ddc, Display};
use futures::stream::StreamExt;
use tokio_udev::{AsyncMonitorSocket, Event, MonitorBuilder};

fn get_attribute<T: FromStr>(event: &Event, attr: &str) -> Option<T> {
    event
        .attribute_value(attr)
        .and_then(OsStr::to_str)
        .and_then(|s| s.parse().ok())
}

fn get_brightness(event: &Event) -> Option<u16> {
    let brightness: u16 = get_attribute(event, "brightness")?;
    let max_brightness: u16 = get_attribute(event, "max_brightness")?;
    Some(brightness * 100 / max_brightness)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let mut brightness: u16 = 0;
    let mut monitor = AsyncMonitorSocket::try_from(
        MonitorBuilder::new()?
            .match_subsystem("backlight")?
            .match_subsystem("drm")?
            .listen()?,
    )?;
    while let Some(event) = monitor.next().await {
        let event = event?;
        match event.device().subsystem().and_then(OsStr::to_str) {
            Some("drm") => {} // refresh
            Some("backlight") => {
                let Some(new_brightness) = get_brightness(&event) else {
                    continue;
                };
                if new_brightness == brightness {
                    continue;
                }
                brightness = new_brightness;
            }
            _ => continue,
        }
        'retry: for _ in 0..3 {
            for mut display in Display::enumerate() {
                if display.handle.set_vcp_feature(0x10, brightness).is_err() {
                    continue 'retry;
                };
            }
            break;
        }
    }
    Ok(())
}
