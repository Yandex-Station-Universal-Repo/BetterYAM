# BetterYAM

BetterYAM contains the `byam` HTTP client and per-device BetterYAMd daemons.
The installer also installs `yam` as a compatibility alias.
Each supported station model owns its API server, Web Panel, and hardware
implementation in its device directory. The first device package is
[`yandexmini/`](./yandexmini/).

Device builds are listed in [`manifest.json`](./manifest.json). Run
`sh tools/build-daemons.sh` with the AArch64 musl target toolchain installed (or
`BUILD_WITH_DOCKER=1 sh tools/build-daemons.sh`) to build every manifest entry
and generate `dist/manifest.json` and `dist/SHA256SUMS`.

Pushing a `v*` tag runs the release workflow. It builds the CLI for Linux
x86_64, macOS Apple Silicon and Windows x86_64, builds the device daemons,
generates checksums, and publishes all release assets. The tag version should
match the version in `Cargo.toml`.

## Installing on a station

On Linux x86_64 or macOS Apple Silicon, install Android Platform Tools and
connect the station with ADB, then run the installer:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.sh | sh
```

On Windows, run PowerShell as a user with ADB installed and use:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.ps1 | iex
```

The installer deploys the daemon supported by the connected station and
installs the CLI on the computer. The primary CLI command is `byam`; `yam` remains available as a compatibility
alias. On macOS and Linux, verify the installation with:

```sh
~/.local/bin/byam --help
```

On macOS and Linux the CLI is installed in `~/.local/bin`. If `byam` is not
found, run it by its full path or add that directory to `PATH`. For zsh, for
example:

```sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

After updating `PATH`, run `byam --help`.

To install a specific release, pass its version:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.sh | sh -s -- 0.1.18
```

Alternatively, from a local repository clone run `sh install.sh` (or
`sh install.sh 0.1.18`). The installer requires `adb`. It selects a supported
connected station automatically, prompts when multiple stations are connected,
or asks for the station IP if none is connected yet.

The installer reads the station hostname with `adb shell uname -n`, removes a
terminal device ID when it is a separate four-character uppercase
alphanumeric segment, normalizes the remaining model name, then matches it
against `models` in the release `manifest.json`. For example:

| Hostname from `uname -n` | Model used for daemon selection |
| --- | --- |
| `Yandex-Station-Midi-44QK` | `Yandex-Station-Midi` |
| `Yandex-Mini3-J3MK` | `Yandex-Mini3` |
| `YandexStationMini_729E` | `YandexStationMini` |
| `Yandex-Lite-gen2-JBDC` | `Yandex-Lite-gen2` |

The `gen2` model suffix is retained; only the final four-character ID segment
is removed. The Mini daemon currently lists `yandexmini`,
`yandexstationmini`, and `yandexstationmini_c190` as model aliases. Midi,
Mini3, and Lite Gen 2 do not have daemon entries yet, so the installer rejects
them instead of deploying the Mini daemon.

To add another station model, implement its daemon in a device directory,
add its build target and exact normalized model aliases to root
[`manifest.json`](./manifest.json), then build and publish that daemon asset
with the release manifest and checksums. Add an alias only when that device
can safely use the listed daemon; do not point an unsupported model at an
existing daemon. See the [device development guide](./docs/ADDING_DEVICES.md)
for the full implementation checklist and a ready-to-use coding-agent prompt,
and [deployment instructions](./docs/DEPLOYMENT.md) for the installer and
release process.

The Mini daemon implements its six-pixel RGB LED ring, read-only Linux input
events when the corresponding kernel devices are accessible, and ALSA Master
volume when the mixer control is available. Bluetooth status and power are
read from rfkill/BSA state; power uses the station control script and is
disabled while `quasar_launcher2` or `maind` is running. Unverified Bluetooth
operations remain unavailable. Wi-Fi networks are listed, added and
forgotten through the station's own wpa_supplicant (`wpa_cli`); an
experimental setup access point lets a phone pick the Wi-Fi when the station
has none (see [docs/DEPLOYMENT.md](./docs/DEPLOYMENT.md)). Other
documented YAM APIs remain present and report unavailable until implemented
for this device.
