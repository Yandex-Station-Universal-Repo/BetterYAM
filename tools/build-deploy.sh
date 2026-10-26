#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/manifest.json"
DEVICE_ROOT=/data/quazar/betteryam
DEVICE_ID=
ADB_SERIAL=${ADB_SERIAL:-}
USE_DOCKER=0

usage() {
  cat <<'USAGE'
Build and deploy one BetterYAM device daemon to an ADB-connected station.

Usage:
  tools/build-deploy.sh [DEVICE_ID] [ADB_SERIAL]
  tools/build-deploy.sh --device DEVICE_ID [--serial ADB_SERIAL] [--docker]

If DEVICE_ID is omitted, select it interactively. If ADB_SERIAL is omitted,
the script selects the only connected ADB device or prompts when there are
several. BUILD_WITH_DOCKER=1 is equivalent to --docker.
USAGE
}

while (($#)); do
  case "$1" in
    --device)
      (($# >= 2)) || { echo "--device requires an ID." >&2; exit 2; }
      DEVICE_ID=$2
      shift 2
      ;;
    --serial)
      (($# >= 2)) || { echo "--serial requires an ADB serial." >&2; exit 2; }
      ADB_SERIAL=$2
      shift 2
      ;;
    --docker)
      USE_DOCKER=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
    *)
      if [[ -z "$DEVICE_ID" ]]; then
        DEVICE_ID=$1
      elif [[ -z "$ADB_SERIAL" ]]; then
        ADB_SERIAL=$1
      else
        echo "Unexpected argument: $1" >&2
        usage >&2
        exit 2
      fi
      shift
      ;;
  esac
done

for tool in cargo python3 adb; do
  command -v "$tool" >/dev/null || { echo "$tool is required." >&2; exit 1; }
done

DEVICE_ENTRIES=()
while IFS= read -r entry; do
  [[ -n "$entry" ]] && DEVICE_ENTRIES+=("$entry")
done < <(python3 - "$MANIFEST" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
if manifest.get("schema_version") != 1 or not isinstance(manifest.get("devices"), list):
    raise SystemExit("unsupported or invalid device manifest")
for entry in manifest["devices"]:
    print(f"{entry['device']}\t{', '.join(entry.get('models', []))}")
PY
)
if ((${#DEVICE_ENTRIES[@]} == 0)); then
  echo "No device daemons are listed in manifest.json." >&2
  exit 1
fi

if [[ -z "$DEVICE_ID" ]]; then
  if [[ ! -r /dev/tty ]]; then
    echo "Device selection requires a terminal or a DEVICE_ID argument." >&2
    exit 1
  fi
  echo "Available device daemons:"
  for index in "${!DEVICE_ENTRIES[@]}"; do
    printf '  %d) %s\n' "$((index + 1))" "${DEVICE_ENTRIES[index]}"
  done
  read -r -p "Select device by number or ID: " selection </dev/tty
  if [[ "$selection" =~ ^[0-9]+$ ]] && ((selection >= 1 && selection <= ${#DEVICE_ENTRIES[@]})); then
    DEVICE_ID=${DEVICE_ENTRIES[selection - 1]%%$'\t'*}
  else
    DEVICE_ID=$selection
  fi
fi

if ! printf '%s\n' "${DEVICE_ENTRIES[@]}" | cut -f1 | grep -Fxq -- "$DEVICE_ID"; then
  echo "Unknown device ID: $DEVICE_ID" >&2
  exit 1
fi

if [[ -z "$ADB_SERIAL" ]]; then
  CONNECTED_DEVICES=()
  while IFS= read -r serial; do
    [[ -n "$serial" ]] && CONNECTED_DEVICES+=("$serial")
  done < <(adb devices | awk 'NR > 1 && $2 == "device" { print $1 }')
  case ${#CONNECTED_DEVICES[@]} in
    0)
      echo "No ADB device connected. Connect the station or pass --serial." >&2
      exit 1
      ;;
    1) ADB_SERIAL=${CONNECTED_DEVICES[0]} ;;
    *)
      if [[ ! -r /dev/tty ]]; then
        echo "Multiple ADB devices connected; pass --serial to select one." >&2
        printf '  %s\n' "${CONNECTED_DEVICES[@]}" >&2
        exit 1
      fi
      echo "Connected ADB devices:"
      for index in "${!CONNECTED_DEVICES[@]}"; do
        printf '  %d) %s\n' "$((index + 1))" "${CONNECTED_DEVICES[index]}"
      done
      read -r -p "Select ADB device by number or serial: " selection </dev/tty
      if [[ "$selection" =~ ^[0-9]+$ ]] && ((selection >= 1 && selection <= ${#CONNECTED_DEVICES[@]})); then
        ADB_SERIAL=${CONNECTED_DEVICES[selection - 1]}
      else
        ADB_SERIAL=$selection
      fi
      ;;
  esac
fi

if [[ $(adb -s "$ADB_SERIAL" get-state 2>/dev/null) != device ]]; then
  echo "ADB device $ADB_SERIAL is not online." >&2
  exit 1
fi

STATION_HOSTNAME=$(adb -s "$ADB_SERIAL" shell uname -n | tr -d '\r')
if ! python3 - "$MANIFEST" "$DEVICE_ID" "$STATION_HOSTNAME" <<'PY'
import json
import re
import sys

manifest_path, device_id, hostname = sys.argv[1:]
entry = next(
    (item for item in json.load(open(manifest_path, encoding="utf-8"))["devices"]
     if item["device"] == device_id),
    None,
)
if entry is None:
    raise SystemExit(f"unknown daemon ID: {device_id}")

model = re.sub(r"[-_][A-Z0-9]{4}$", "", hostname)
model_id = re.sub(r"[^a-z0-9_-]", "-", model.lower())
model_id = re.sub(r"-+", "-", model_id).strip("-")
aliases = entry.get("models", [device_id])
if model_id not in aliases:
    print(
        f"Daemon {device_id} does not declare station model {model!r} "
        f"(normalized ID {model_id!r}); declared models: {', '.join(aliases)}",
        file=sys.stderr,
    )
    raise SystemExit(1)
print(f"Matched {device_id} to station model {model!r}.")
PY
then
  echo "Refusing to deploy a daemon that does not match this station." >&2
  exit 1
fi

WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/betteryam-build-deploy.XXXXXX")
trap 'rm -rf -- "$WORK_DIR"' EXIT

run_remote_script() {
  remote_script="/tmp/betteryam-build-deploy-$$.sh"
  local_script="$WORK_DIR/remote-script.sh"
  cat >"$local_script"
  adb -s "$ADB_SERIAL" push "$local_script" "$remote_script" >/dev/null
  adb -s "$ADB_SERIAL" shell \
    "sh '$remote_script'; result=\$?; rm -f '$remote_script'; exit \$result"
}

echo "Building stripped daemon for $DEVICE_ID..."
if [[ "$USE_DOCKER" == 1 || "${BUILD_WITH_DOCKER:-0}" == 1 ]]; then
  BUILD_WITH_DOCKER=1 sh "$ROOT/tools/build-daemons.sh" \
    "$MANIFEST" "$WORK_DIR/dist" "$DEVICE_ID"
else
  sh "$ROOT/tools/build-daemons.sh" \
    "$MANIFEST" "$WORK_DIR/dist" "$DEVICE_ID"
fi

ASSET=$(python3 - "$WORK_DIR/dist/manifest.json" <<'PY'
import json
import sys

devices = json.load(open(sys.argv[1], encoding="utf-8"))["devices"]
if len(devices) != 1:
    raise SystemExit("expected exactly one daemon in the build manifest")
print(devices[0]["asset"])
PY
)
LOCAL_BINARY="$WORK_DIR/dist/assets/$ASSET"
if [[ ! -f "$LOCAL_BINARY" ]]; then
  echo "Build did not produce expected daemon asset: $ASSET" >&2
  exit 1
fi

echo "Built daemon:"
ls -lh "$LOCAL_BINARY"
echo "Remounting the station filesystem and preparing daemon upload..."
run_remote_script <<'REMOTE'
set -eu
mount -o remount,rw /
mount | grep ' on / ' | grep -q '(rw' || {
    echo "Root filesystem is not writable after remount." >&2
    exit 1
}
mkdir -p /data/quazar/betteryam/bin /data/quazar/betteryam/data
REMOTE
echo "Uploading daemon and startup hook to $ADB_SERIAL..."
adb -s "$ADB_SERIAL" push "$LOCAL_BINARY" "$DEVICE_ROOT/bin/betteryamd.new"
adb -s "$ADB_SERIAL" push "$ROOT/tools/betteryamd.init" "$DEVICE_ROOT/betteryamd.init.new"

echo "Replacing and restarting BetterYAMd..."
run_remote_script <<'REMOTE'
set -eu
root=/data/quazar/betteryam
binary=$root/bin/betteryamd
staged=$root/bin/betteryamd.new
backup=$root/bin/betteryamd.previous
pidfile=$root/betteryamd.pid
init=/etc/init.d/S99zzbetteryam
staged_init=$root/betteryamd.init.new

stop_daemon() {
    if [ -x "$init" ]; then
        "$init" stop
    elif [ -f "$pidfile" ]; then
        pid=$(cat "$pidfile")
        case "$pid" in
            ''|*[!0-9]*) echo "Invalid BetterYAMd pid file" >&2; return 1 ;;
        esac
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid"
            attempt=0
            while kill -0 "$pid" 2>/dev/null && [ "$attempt" -lt 50 ]; do
                sleep 0.1
                attempt=$((attempt + 1))
            done
            if kill -0 "$pid" 2>/dev/null; then
                echo "BetterYAMd did not stop" >&2
                return 1
            fi
        fi
        rm -f "$pidfile"
    fi
}

start_daemon() {
    "$init" start
}

daemon_running() {
    attempt=0
    while [ "$attempt" -lt 20 ]; do
        pid=$(cat "$pidfile" 2>/dev/null || true)
        case "$pid" in
            ''|*[!0-9]*) ;;
            *)
                if kill -0 "$pid" 2>/dev/null; then
                    return 0
                fi
                ;;
        esac
        sleep 0.1
        attempt=$((attempt + 1))
    done
    return 1
}

if [ -e "$init" ] && ! grep -q '^# Managed by BetterYAM$' "$init"; then
    echo "Refusing to replace unmanaged init script: $init" >&2
    exit 1
fi
chmod 0755 "$staged"
chmod 0755 "$staged_init"
stop_daemon
if [ -f "$binary" ]; then
    cp "$binary" "$backup"
fi
mv "$staged" "$binary"
mv "$staged_init" "$init"

if start_daemon && daemon_running; then
    echo "BetterYAMd restarted successfully."
    exit 0
fi

echo "New daemon failed to start; restoring previous binary." >&2
stop_daemon || true
if [ -f "$backup" ]; then
    mv "$backup" "$binary"
    if start_daemon && daemon_running; then
        echo "Previous daemon restored." >&2
    else
        echo "Could not restart the previous daemon." >&2
    fi
fi
exit 1
REMOTE

echo "Deployment complete: $ADB_SERIAL ($DEVICE_ROOT/bin/betteryamd); boot startup installed at /etc/init.d/S99zzbetteryam"
