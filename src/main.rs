use std::ffi::OsStr;
use std::io;
use std::str::FromStr;

use ddc_hi::{Ddc, Display};
use futures::{StreamExt, TryStreamExt};
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
    AsyncMonitorSocket::try_from(
        MonitorBuilder::new()?
            .match_subsystem("backlight")?
            .listen()?,
    )?
    .try_filter_map(|event| async move { Ok(get_brightness(&event)) })
    .try_for_each(|new_brightness| async move {
        if new_brightness != brightness {
            brightness = new_brightness;
            for mut display in Display::enumerate() {
                display.handle.set_vcp_feature(0x10, brightness).unwrap();
            }
        }
        Ok(())
    })
    .await
}
