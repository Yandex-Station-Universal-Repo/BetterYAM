# BetterYAM

BetterYAM contains the `yam` HTTP client and per-device BetterYAMd daemons.
Each supported station model owns its API server, Web Panel, and hardware
implementation in its device directory. The first device package is
[`yandexmini/`](./yandexmini/).

Device builds are listed in [`manifest.json`](./manifest.json).
Run `sh tools/build-daemons.sh` with the AArch64 musl target toolchain installed
(or `BUILD_WITH_DOCKER=1 sh tools/build-daemons.sh`) to build every manifest entry and generate `dist/manifest.json` and
`dist/SHA256SUMS`. Release `manifest.json` and each generated daemon asset.
The installer uses the station's `uname -n` value to select a matching device
record; unknown models are rejected rather than assigned another daemon.

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
