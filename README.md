# BetterYAM

BetterYAM contains the `yam` HTTP client and per-device BetterYAMd daemons.
Each supported station model owns its API server, Web Panel, and hardware
implementation in its device directory. The first device package is
[`yandexmini/`](./yandexmini/).

Device builds are listed in [`manifest.json`](./manifest.json).
Run `sh tools/build-daemons.sh` with the AArch64 musl target toolchain installed
(or `BUILD_WITH_DOCKER=1 sh tools/build-daemons.sh`) to build every manifest entry and generate `dist/manifest.json` and
`dist/SHA256SUMS`. Release `manifest.json` and each generated daemon asset.
Pushing a `v*` tag runs the release workflow, which builds the CLI and device
daemons, generates checksums, and publishes the installer assets. The tag
version should match the version in `Cargo.toml`.
## Installing on a station

On Linux or macOS, connect the station with ADB, then run the installer:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.sh | sh
```

To install a specific release, pass its version:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.sh | sh -s -- 0.1.14
```

Alternatively, from a local repository clone run `sh install.sh` (or
`sh install.sh 0.1.14`). The script requires `adb` and a connected station; if
multiple supported stations are connected, it prompts you to select one.

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
existing daemon. See [deployment instructions](./docs/DEPLOYMENT.md) for the
installer and release process.

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
