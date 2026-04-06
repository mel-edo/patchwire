PREFIX ?= $(HOME)/.local
SYSTEMD_DIR ?= $(HOME)/.config/systemd/user
APPS_DIR ?= $(HOME)/.local/share/applications

build:
	cargo build --release

install: build
	install -Dm755 target/release/patchwire $(PREFIX)/bin/patchwire
	install -Dm755 target/release/patchwire-gtk $(PREFIX)/bin/patchwire-gtk
	install -Dm644 data/patchwire.service $(SYSTEMD_DIR)/patchwire.service
	install -Dm644 data/patchwire.desktop $(APPS_DIR)/patchwire.desktop
	systemctl --user daemon-reload
	systemctl --user enable patchwire
	@echo "installed succesfully, launch Patchwire from your app menu."

uninstall:
	systemctl --user disable --now patchwire || true
	rm -f $(PREFIX)/bin/patchwire
	rm -f $(PREFIX)/bin/patchwire-gtk
	rm -f $(SYSTEMD_DIR)/patchwire.service
	rm -f $(APPS_DIR)/patchwire.desktop
	systemctl --user daemon-reload

.PHONY: build install uninstall