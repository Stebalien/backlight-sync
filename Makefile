CARGO_TARGET_DIR ?= target
PREFIX           ?= /usr/local
LIBDIR           ?= $(PREFIX)/lib
LIBEXECDIR       ?= $(LIBDIR)

export CARGO_TARGET_DIR

BIN = $(CARGO_TARGET_DIR)/release/backlight-sync

build: $(BIN) contrib/backlight-syncd.service

$(BIN): .
	cargo build --release

%: %.in
	m4 -DBINDIR="$(BINDIR)" \
		-DLIBEXECDIR="$(LIBEXECDIR)" \
		-DPREFIX="$(PREFIX)" \
		$< > $@
install: contrib/backlight-syncd.service
	install -Dm755 $(CARGO_TARGET_DIR)/release/backlight-sync $(DESTDIR)$(LIBEXECDIR)/backlight-syncd
	install -Dm644 contrib/backlight-syncd.service $(DESTDIR)$(LIBDIR)/systemd/system/backlight-syncd.service

install-udev-rules:
	install -Dm644 contrib/i2c.sysusers.conf $(DESTDIR)$(LIBDIR)/sysusers.d/i2c.conf
	install -Dm644 contrib/i2c.udev.rules $(DESTDIR)$(LIBDIR)/udev/rules.d/50-i2c.rules

clean:
	rm -fr $(CARGO_TARGET_DIR)
	rm contrib/backlight-syncd.service


.PHONY: build install
