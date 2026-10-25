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

The Mini daemon's currently implemented hardware capability is its six-pixel
RGB LED ring. Other documented YAM APIs remain present and report unavailable
until implemented for this device.
