use std::ffi::OsStr;
use std::io;

use ddc_hi::{Ddc, Display};
use futures::StreamExt;
use tokio_udev::{AsyncMonitorSocket, MonitorBuilder};

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let mut monitor: AsyncMonitorSocket = MonitorBuilder::new()?
        .match_subsystem("backlight")?
        .listen()?
        .try_into()?;

    let mut old_value: u16 = 0;
    while let Some(event) = monitor.next().await {
        let event = event?;
        let Some(brightness): Option<u16> = event
            .attribute_value("brightness")
            .and_then(OsStr::to_str)
            .and_then(|s| s.parse().ok())
        else {
            continue;
        };
        let Some(max_brightness): Option<u16> = event
            .attribute_value("max_brightness")
            .and_then(OsStr::to_str)
            .and_then(|s| s.parse().ok())
        else {
            continue;
        };
        let target = brightness * 100 / max_brightness;
        if target == old_value {
            continue;
        }
        old_value = target;

        for mut display in Display::enumerate() {
            display.handle.set_vcp_feature(0x10, target).unwrap();
        }
    }

    Ok(())
}
