# Backlight Sync Daemon

This daemon automatically propagates the current brightness level from a laptop screen to attached monitors via DDC. It doesn't support any configuration and has no way of knowing the brightness curves of your various monitors, so I make no guarantees. However, it works great for my laptop/monitor setup.

## Install

If you're using Arch Linux, you should install it from the AUR via the [backlight-sync-git](https://aur.archlinux.org/packages/backlight-sync-git) package.

Otherwise, you can manually install it by running `make` followed by `sudo make install`. If you plan to use the built-in `backlight-syncd.service` unmodified and your `/dev/i2c-*` devices are not readable/writable by an `i2c` group, you'll need to run `sudo make install-udev-rules` then reboot.

## Using

Run `systemctl enable --now backlight-syncd.service` to enable and start the service.

If you want to run the service without systemd, you'll need to make sure to run it as a user/group with read/write access to `/dev/i2c-*`. It needs no other permissions.

## License

This project is licensed under the GPL-v3, copyright Steven Allen 2024-2025.
