# Install helper for distributions and local installs.
#   make            # release build
#   make install PREFIX=/usr/local DESTDIR=
PREFIX ?= /usr/local
DESTDIR ?=
APP_ID = io.github.rustnior.Rustnior
CARGO_TARGET ?= target/release

.PHONY: build install uninstall flatpak-sources

build:
	cargo build --release --locked

install:
	install -Dm755 $(CARGO_TARGET)/rustnior $(DESTDIR)$(PREFIX)/bin/rustnior
	install -Dm644 data/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).metainfo.xml $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
	install -Dm644 data/icons/hicolor/scalable/apps/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 data/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/rustnior \
	      $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop \
	      $(DESTDIR)$(PREFIX)/share/metainfo/$(APP_ID).metainfo.xml \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg

# Vendored crate list for the offline Flatpak build (needs flatpak-cargo-generator.py
# from https://github.com/flatpak/flatpak-builder-tools).
flatpak-sources:
	python3 flatpak-cargo-generator.py Cargo.lock -o build-aux/cargo-sources.json
