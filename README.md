# BetterYAM

BetterYAM is a YAM-compatible plugin runtime. The `yam` CLI and Web Panel are
HTTP clients; each supported station model has its own `betteryamd` release
asset and hardware implementation.

The installer reads the station model with `uname -n` and selects
`betteryamd-<model-id>-aarch64-linux` from this repository's release. The
`<model-id>` is the lower-case hostname with unsupported characters replaced
by hyphens. Build/release automation must publish that asset and include it in
`SHA256SUMS` for each supported model.
