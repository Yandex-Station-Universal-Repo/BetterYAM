# YAM agent guide

## Repository layout

- `src/main.rs` builds `betteryamd`, the HTTP API, package manager and Lua bridge.
- `src/bin/yam.rs` builds `yam`, the command-line client.
- `web/index.html` is embedded in the binary with `include_str!`.
- `examples/` contains source trees for `.yap` packages.
- `docs/` is the public developer documentation.
- `install.sh` installs the `yam` CLI and the model-specific `betteryamd` Linux release asset.
- `tools/convert_apkg.py` converts an apkg repository package into `.yap`.

## Required checks

Run these after changing Rust or shell files:

```sh
cargo fmt --check
cargo check
sh -n install.sh
```

For a Station Lite / Micro target, build the AArch64 musl binary in Docker:

```sh
docker run --rm -v "$PWD":/home/rust/src -w /home/rust/src messense/rust-musl-cross:aarch64-musl cargo build --release
```

Do not commit `target/`, package archives, device data, credentials or logs.

## Runtime conventions

- Do not expose a hardware operation to Lua without a manifest permission.
- Advertise a feature in `DeviceProfile` only when its adapter is detectable and usable.
- Keep path handling safe: package paths are relative and must reject parent components.
- Long-running Lua packages must be cancellable through the shared cancellation token and should call `alice.sleep` or `alice.set` regularly.
- Never put model-specific hardware code in the shared runtime. Each model's `betteryamd` release must implement its hardware capabilities and report availability in the Device API profile.

## Package conventions

- A package is a ZIP archive with `.yap` extension and a root `plugin.toml`.
- Use a lowercase ID containing only letters, digits, `_` and `-`.
- Declare the smallest possible permission set.
- Add `[compatibility]` feature requirements for every hardware-specific API.
- Test package installation through `POST /api/plugins`, then test start and stop through the Web UI or CLI.
