# YAM agent guide

## Repository layout

- `betteryam-runtime/` contains the shared daemon HTTP API, package manager,
  Lua bridge and hardware adapter traits.
- `yandexmini/` and `yandexmicro/` contain daemon entry points and
  device-specific hardware adapters.
- `betteryam-runtime/src/web/` contains the UI embedded in each daemon.
- `src/bin/yam.rs` builds `byam`, the command-line client.
- `examples/` contains source trees for `.yap` packages.
- `docs/` is the public developer documentation.
- `docs/ADDING_DEVICES.md` guides contributors and coding agents through adding a device daemon.
- `manifest.json` lists device daemon packages and build targets.
- `tools/build-daemons.sh` builds all manifest entries and creates release manifest/checksums.
- `install.sh` installs `byam` (with a `yam` compatibility alias) and selects
  the device daemon by `uname -n`.
- `tools/convert_apkg.py` converts an apkg repository package into `.yap`.

## Required checks

Run these after changing Rust or shell files:

```sh
cargo fmt --all --check
cargo check --workspace
sh -n install.sh
```

For a Station Lite / Micro target, build the AArch64 musl binary with the
installed cross-linker or in Docker:

```sh
sh tools/build-daemons.sh
# Or build only the Lite daemon directly:
docker run --rm \
  -v "$PWD":/home/rust/src \
  -w /home/rust/src \
  messense/rust-musl-cross:aarch64-musl \
  cargo build --release \
    --target aarch64-unknown-linux-musl \
    --manifest-path yandexmicro/Cargo.toml \
    --bin betteryamd-yandexmicro
```

Do not commit `target/`, package archives, device data, credentials or logs.

## Runtime conventions

- Do not expose a hardware operation to Lua without a manifest permission.
- Advertise a feature in `DeviceProfile` only when its adapter is detectable and usable.
- Keep path handling safe: package paths are relative and must reject parent components.
- Long-running Lua packages must be cancellable through the shared token.
  They should call `alice.sleep` or another appropriate API regularly.
- Keep the shared HTTP API and plugin runtime in `betteryam-runtime/`; keep
  model-specific hardware drivers and adapter configuration in that device's
  directory.
- Report unsupported device features as unavailable; do not silently route them
  to another device daemon.

## Package conventions

- A package is a ZIP archive with `.yap` extension and a root `plugin.toml`.
- Use a lowercase ID containing only letters, digits, `_` and `-`.
- Declare the smallest possible permission set.
- Add `[compatibility]` feature requirements for every hardware-specific API.
- Test package installation through `POST /api/plugins`, then test start and
  stop through the Web UI or CLI.
