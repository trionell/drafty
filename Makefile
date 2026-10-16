# Linux build and install. On macOS use scripts/bundle-macos.sh instead.
#
#   make                                   build the release binary
#   make install                           install for this user, into ~/.local
#   sudo make install PREFIX=/usr/local    install for everyone (run `make` first)
#   make uninstall                         remove what install put there

PREFIX ?= $(HOME)/.local
BIN := target/release/drafty
SHARE := $(DESTDIR)$(PREFIX)/share

.PHONY: all build test install uninstall refresh-caches

all: build

build:
	cargo build --release

test:
	cargo test

# Doesn't build, so `sudo make install` never runs cargo as root.
install:
	@test -f $(BIN) || { echo "No release binary. Run 'make' first."; exit 1; }
	install -Dm755 $(BIN) $(DESTDIR)$(PREFIX)/bin/drafty
	install -Dm644 assets/drafty.desktop $(SHARE)/applications/drafty.desktop
	install -Dm644 assets/icon.png $(SHARE)/icons/hicolor/256x256/apps/drafty.png
	install -Dm644 assets/icon.svg $(SHARE)/icons/hicolor/scalable/apps/drafty.svg
	install -Dm644 assets/drafty-mime.xml $(SHARE)/mime/packages/drafty.xml
	@$(MAKE) --no-print-directory refresh-caches

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/drafty
	rm -f $(SHARE)/applications/drafty.desktop
	rm -f $(SHARE)/icons/hicolor/256x256/apps/drafty.png
	rm -f $(SHARE)/icons/hicolor/scalable/apps/drafty.svg
	rm -f $(SHARE)/mime/packages/drafty.xml
	@$(MAKE) --no-print-directory refresh-caches

# Packagers set DESTDIR and let the system's hooks rebuild these caches.
refresh-caches:
ifeq ($(DESTDIR),)
	-update-mime-database $(SHARE)/mime >/dev/null 2>&1
	-update-desktop-database $(SHARE)/applications >/dev/null 2>&1
	-gtk-update-icon-cache -q -t $(SHARE)/icons/hicolor >/dev/null 2>&1
endif
