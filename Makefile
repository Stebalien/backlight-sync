CARGO_TARGET_DIR ?= target
PREFIX           ?= /usr/local
LIBEXECDIR       ?= $(PREFIX)/lib

export CARGO_TARGET_DIR

BIN = $(CARGO_TARGET_DIR)/release/backlight-sync

build: $(BIN) backlight-syncd.service

$(BIN): .
	cargo build --release

%: %.in
	m4 -DBINDIR="$(BINDIR)" \
		-DLIBEXECDIR="$(LIBEXECDIR)" \
		-DPREFIX="$(PREFIX)" \
		$< > $@
install:
	install -Dm755 $(CARGO_TARGET_DIR)/release/backlight-sync $(DESTDIR)$(LIBEXECDIR)/backlight-syncd
	install -Dm644 backlight-syncd.service /usr/lib/systemd/system/backlight-syncd.service

clean:
	rm -fr $(CARGO_TARGET_DIR)
	rm backlight-syncd.service


.PHONY: build install
