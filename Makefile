build:
	cargo build --release
install:
	install -Dm755 target/release/backlight-sync /usr/lib/backlight-syncd
	install -Dm644 backlight-syncd.service /usr/lib/systemd/system/backlight-syncd.service
