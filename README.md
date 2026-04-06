# Patchwire

**Route audio to multiple outputs simultaneously on Linux.**

Play through your headphones and speakers at the same time. Patchwire watches the PipeWire graph and manages the links for you.

[![AUR version](https://img.shields.io/aur/version/patchwire)](https://aur.archlinux.org/packages/patchwire)
[![AUR version](https://img.shields.io/aur/version/patchwire-bin)](https://aur.archlinux.org/packages/patchwire-bin)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/rust-orange.svg)](https://www.rust-lang.org/)

---

## Overview

Every time you connect a Bluetooth device or want to route audio to multiple outputs, you'd normally have to manually wire ports in Helvum or qpwgraph. Patchwire does this automatically.

- Toggle any sink on or off as a secondary output
- Control volume per device
- Change your default output device from the UI
- State persists across reboots
- Daemon starts automatically on first launch

---

## How it works

Patchwire runs a background daemon that connects directly to PipeWire via `libpipewire`. It watches the graph for new devices and manages `pw::Link` objects between your default sink's monitor ports and the playback ports of any enabled secondary sinks.

The GTK4 UI talks to the daemon over D-Bus (`com.patchwire.Daemon`). The CLI does the same.

---

## Installation (for Arch based distros)

### Via AUR
```bash
yay -S patchwire      # building from source
yay -S patchwire-bin  # prebuilt binary
```

### Manual
To build from source, you will need the standard Rust toolchain and the GTK4 development libraries.

**1. Install dependencies**

Install rust toolchain via [rustup](https://rustup.rs/)

```bash
sudo pacman -S --needed pipewire pipewire-audio wireplumber gtk4 libadwaita
```

**2. Clone and Build**

```bash
git clone https://github.com/mel-edo/patchwire
cd patchwire
make install
```

---

## Usage

### GUI
Launch Patchwire from your app menu. The daemon starts automatically.

### CLI
If you installed the app via `make install` or the AUR, the daemon runs automatically in the background via systemd as a user service.

> If you are running the project manually without systemd, you will need to run `patchwire daemon` in a separate terminal first.

You can control it directly from your terminal:
```bash
patchwire list              # show all sinks
patchwire toggle <sink>     # toggle routing for a sink
patchwire volume <sink> 75  # set volume to 75%
patchwire help              # show all commands
```

The daemon runs as a systemd user service after `make install` and starts automatically on login.

---

## Building from source
```bash
cargo build --release
```

Binaries in `target/release/`.

## Contributing

Contributions are welcome. Feel free to open an issue or a PR.

## License

GPL-3.0 - see [LICENSE](LICENSE)
