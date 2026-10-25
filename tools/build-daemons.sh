#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
MANIFEST=${1:-"$ROOT/manifest.json"}
DIST_DIR=${2:-"$ROOT/dist"}

command -v cargo >/dev/null 2>&1 || {
  echo "cargo is required to build device daemons." >&2
  exit 2
}
command -v python3 >/dev/null 2>&1 || {
  echo "python3 is required to read the device manifest." >&2
  exit 2
}

python3 - "$ROOT" "$MANIFEST" "$DIST_DIR" <<'PY'
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1]).resolve()
manifest_path = Path(sys.argv[2]).resolve()
dist_dir = Path(sys.argv[3]).resolve()
manifest = json.loads(manifest_path.read_text())
if manifest.get("schema_version") != 1 or not isinstance(manifest.get("devices"), list):
    raise SystemExit("manifest must have schema_version=1 and a devices array")
if not manifest["devices"]:
    raise SystemExit("manifest contains no device daemon entries")

target_dir = Path(os.environ.get("CARGO_TARGET_DIR", root / "target"))
if not target_dir.is_absolute():
    target_dir = root / target_dir
assets_dir = dist_dir / "assets"
assets_dir.mkdir(parents=True, exist_ok=True)
release_devices = []

for entry in manifest["devices"]:
    device = entry["device"]
    target = entry["target"]
    binary = entry["binary"]
    asset = entry["asset"]
    cargo_manifest = (root / entry["manifest_path"]).resolve()
    if not cargo_manifest.is_relative_to(root) or not cargo_manifest.is_file():
        raise SystemExit(f"invalid manifest_path for {device}: {cargo_manifest}")
    if not all(isinstance(item, str) and item for item in (device, target, binary, asset)):
        raise SystemExit(f"invalid required fields for {device}")
    if Path(asset).name != asset:
        raise SystemExit(f"asset must be a filename: {asset}")
    models = entry.get("models", [device])
    if not models or not all(isinstance(model, str) and model for model in models):
        raise SystemExit(f"invalid models list for {device}")

    cargo_command = [
        "cargo", "build", "--release", "--target", target,
        "--manifest-path", str(cargo_manifest), "--bin", binary,
    ]
    if os.environ.get("BUILD_WITH_DOCKER") == "1":
        image_target = target.replace("-unknown-linux-", "-")
        docker_manifest = Path("/home/rust/src") / cargo_manifest.relative_to(root)
        command = [
            "docker", "run", "--rm",
            "-v", f"{root}:/home/rust/src",
            "-w", "/home/rust/src",
            f"messense/rust-musl-cross:{image_target}",
            "cargo", "build", "--release", "--target", target,
            "--manifest-path", str(docker_manifest), "--bin", binary,
        ]
    else:
        command = cargo_command
    print(f"Building {device}: {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=root, check=True)

    built_binary = target_dir / target / "release" / binary
    if not built_binary.is_file():
        raise SystemExit(f"cargo did not produce daemon binary: {built_binary}")
    destination = assets_dir / asset
    shutil.copy2(built_binary, destination)
    destination.chmod(destination.stat().st_mode | 0o111)
    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    release_devices.append({
        "device": device,
        "models": models,
        "target": target,
        "asset": asset,
        "filepath": str(destination.relative_to(dist_dir)),
        "sha256sum": digest,
    })

(dist_dir / "manifest.json").write_text(
    json.dumps({"schema_version": 1, "devices": release_devices}, indent=2) + "\n"
)
(dist_dir / "SHA256SUMS").write_text(
    "".join(
        f"{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n"
        for asset in sorted(assets_dir.iterdir())
        if asset.is_file()
    )
)
print(f"Wrote {dist_dir / 'manifest.json'} and {dist_dir / 'SHA256SUMS'}")
PY
