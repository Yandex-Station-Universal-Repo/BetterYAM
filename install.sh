#!/bin/sh
set -eu

ACTION=install
case "${1:-}" in
  --uninstall|uninstall) ACTION=uninstall; shift ;;
esac
VERSION=${1:-${BETTERYAM_VERSION:-${YAM_VERSION:-latest}}}
BIN_DIR=${2:-${YAM_BIN_DIR:-$HOME/.local/bin}}
REPOSITORY=https://github.com/Yandex-Station-Universal-Repo/BetterYAM
DEVICE_PREFIX=/data/quazar/betteryam

strip_device_id() {
  printf '%s' "$1" | LC_ALL=C sed 's/[-_][A-Z0-9]\{4\}$//'
}

log() {
  printf '[BetterYAM install] %s\n' "$*" >&2
}

if [ "$ACTION" = install ]; then
  case "$(uname -s):$(uname -m)" in
    Linux:x86_64|Linux:amd64) CLI_ASSET=byam-x86_64-unknown-linux-musl ;;
    Darwin:arm64|Darwin:aarch64) CLI_ASSET=byam-aarch64-apple-darwin ;;
    *) echo "Unsupported host platform $(uname -s) $(uname -m)." >&2; exit 3 ;;
  esac
fi

case "$VERSION" in
  latest) RELEASE_URL="$REPOSITORY/releases/latest/download" ;;
  v*) RELEASE_URL="$REPOSITORY/releases/download/$VERSION" ;;
  *) RELEASE_URL="$REPOSITORY/releases/download/v$VERSION" ;;
esac

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/betteryam.XXXXXX")
trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM
run_remote_script() {
  remote_script="/tmp/betteryam-install-$$.sh"
  local_script="$tmpdir/remote-script.sh"
  cat >"$local_script"
  adb -s "$DEVICE" push "$local_script" "$remote_script" </dev/null >/dev/null
  adb -s "$DEVICE" shell \
    "sh '$remote_script'; result=\$?; rm -f '$remote_script'; exit \$result" </dev/null
}
download() {
  url=$1 output=$2
  log "Downloading $(basename "$output") from $url"
  if command -v curl >/dev/null 2>&1; then
    if http_status=$(curl -sS -L --proto '=https,http' --tlsv1.2 \
      --write-out '%{http_code}' -o "$output" "$url"); then
      case "$http_status" in
        2??) log "Downloaded $(basename "$output") (HTTP $http_status)" ;;
        *)
          rm -f "$output"
          log "HTTP $http_status while downloading $(basename "$output") from $url"
          return 1
          ;;
      esac
    else
      curl_status=$?
      rm -f "$output"
      log "curl exited with status $curl_status while downloading $(basename "$output") from $url (HTTP ${http_status:-unknown})"
      return "$curl_status"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if wget -O "$output" "$url"; then
      log "Downloaded $(basename "$output")"
    else
      wget_status=$?
      rm -f "$output"
      log "wget exited with status $wget_status while downloading $(basename "$output") from $url"
      return "$wget_status"
    fi
  else
    echo 'curl or wget is required.' >&2; exit 4
  fi
}
verify() {
  file=$1
  expected=$(awk -v name="$(basename "$file")" '$2 == name {print $1}' "$tmpdir/SHA256SUMS")
  [ -n "$expected" ] || { echo "No checksum for $(basename "$file")" >&2; exit 5; }
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$file" | awk '{print $1}')
  elif command -v openssl >/dev/null 2>&1; then
    actual=$(openssl dgst -sha256 "$file" | awk '{print $NF}')
  else
    echo 'sha256sum or openssl is required.' >&2; exit 5
  fi
  [ "$actual" = "$expected" ] || { echo "Checksum mismatch for $(basename "$file")" >&2; exit 5; }
}

command -v adb >/dev/null 2>&1 || {
  echo 'adb is required to deploy BetterYAMd. Install Android Platform Tools first.' >&2; exit 4;
}

choose_device() {
  devices=$(adb devices </dev/null | awk 'NR > 1 && $2 == "device" {print $1}' | while IFS= read -r serial; do
    arch=$(adb -s "$serial" shell uname -m </dev/null 2>/dev/null | tr -d '\r') || continue
    if [ "$arch" = aarch64 ] || [ "$arch" = arm64 ] || [ "$arch" = armv7l ] || [ "$arch" = armv7 ]; then
      model=$(adb -s "$serial" shell uname -n </dev/null 2>/dev/null | tr -d '\r')
      model=$(strip_device_id "$model")
      printf '%s\t%s\n' "$serial" "${model:-unknown}"
    fi
  done
  )
  [ -n "$devices" ] || return 1
  count=$(printf '%s\n' "$devices" | wc -l | tr -d ' ')
  if [ "$count" -eq 1 ]; then
    printf '%s\n' "$devices" | awk -F '\t' '{print $1}'
    return 0
  fi
  echo 'Select station:' >&2
  printf '%s\n' "$devices" | awk -F '\t' '{printf "  %d) %s (%s)\n", NR, $2, $1}' >&2
  printf 'Number: ' >&2
  IFS= read -r selection </dev/tty
  case "$selection" in
    ''|*[!0-9]*) echo 'Invalid selection.' >&2; return 1 ;;
  esac
  selected=$(printf '%s\n' "$devices" | sed -n "${selection}p")
  [ -n "$selected" ] || { echo 'Invalid selection.' >&2; return 1; }
  printf '%s\n' "$selected" | awk -F '\t' '{print $1}'
}

DEVICE=$(choose_device || true)
if [ -z "$DEVICE" ]; then
  printf 'Station IP address: ' >&2
  IFS= read -r ip </dev/tty
  [ -n "$ip" ] || { echo 'IP address is required.' >&2; exit 6; }
  DEVICE="$ip:5555"
  adb connect "$DEVICE" </dev/null >&2
  sleep 1
  adb -s "$DEVICE" get-state </dev/null >/dev/null
fi

case "$DEVICE" in
  *:*) STATION_IP=${DEVICE%:*} ;;
  *)
    STATION_IP=$(adb -s "$DEVICE" shell 'ip route get 1.1.1.1 2>/dev/null' </dev/null 2>/dev/null | awk '{for (i = 1; i <= NF; i++) if ($i == "src") print $(i + 1)}' | tr -d '\r' | head -n 1)
    if [ -z "$STATION_IP" ]; then
      printf 'Station IP address: ' >&2
      IFS= read -r STATION_IP </dev/tty
    fi
    ;;
esac
[ -n "$STATION_IP" ] || { echo 'IP address is required.' >&2; exit 6; }

if [ "$ACTION" = uninstall ]; then
  run_remote_script <<'REMOTE'
set -eu
mount -o remount,rw /
mount | grep ' on / ' | grep -q '(rw' || {
    echo "Root filesystem is not writable after remount." >&2
    exit 1
}
root=/data/quazar/betteryam
init=/etc/init.d/S99zzbetteryam
if [ -f "$init" ]; then
    if ! grep -q '^# Managed by BetterYAM$' "$init"; then
        echo "Refusing to remove unmanaged init script: $init" >&2
        exit 1
    fi
    "$init" stop
    rm -f "$init"
elif [ -f "$root/betteryamd.pid" ]; then
    pid=$(cat "$root/betteryamd.pid")
    case "$pid" in
        ''|*[!0-9]*) echo "Invalid BetterYAMd pid file" >&2; exit 1 ;;
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
            exit 1
        fi
    fi
fi
rm -rf "$root"
REMOTE
  rm -f "$BIN_DIR/byam" "$BIN_DIR/byam-bin" "$BIN_DIR/yam"
  echo "YAM removed from $BIN_DIR and $DEVICE ($DEVICE_PREFIX)."
  exit 0
fi

command -v python3 >/dev/null 2>&1 || {
  echo 'python3 is required to select the device daemon from manifest.json.' >&2; exit 4;
}

STATION_MODEL=$(adb -s "$DEVICE" shell uname -n </dev/null 2>/dev/null | tr -d '\r')
STATION_MODEL=$(strip_device_id "$STATION_MODEL")
MODEL_ID=$(printf '%s' "$STATION_MODEL" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9_-]/-/g; s/--*/-/g; s/^-//; s/-$//')
[ -n "$MODEL_ID" ] || { echo 'Could not determine a valid station model using uname -n.' >&2; exit 7; }
log "Selected station $DEVICE: hostname model '$STATION_MODEL' (manifest ID '$MODEL_ID')"
log "Using release URL $RELEASE_URL"

download "$RELEASE_URL/SHA256SUMS" "$tmpdir/SHA256SUMS"
download "$RELEASE_URL/manifest.json" "$tmpdir/manifest.json"
download "$RELEASE_URL/$CLI_ASSET" "$tmpdir/$CLI_ASSET"
if ! DAEMON_RECORD=$(python3 - "$tmpdir/manifest.json" "$MODEL_ID" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
matches = [
    device for device in manifest.get("devices", [])
    if sys.argv[2] in device.get("models", [device.get("device", "")])
]
if len(matches) != 1:
    print(f"release manifest matched {len(matches)} daemon entries for model ID {sys.argv[2]}", file=sys.stderr)
    raise SystemExit(1)
entry = matches[0]
for field in ("asset", "sha256sum", "filepath"):
    value = entry.get(field)
    if not isinstance(value, str) or not value:
        raise SystemExit(1)
print("\t".join(entry[field] for field in ("asset", "sha256sum", "filepath")))
PY
); then
  echo "No unique BetterYAMd manifest entry for station model '$STATION_MODEL' ($MODEL_ID)." >&2
  exit 8
fi
IFS="$(printf '\t')" read -r DAEMON_ASSET DAEMON_SHA256 DAEMON_FILEPATH <<EOF
$DAEMON_RECORD
EOF
log "Selected daemon asset $DAEMON_ASSET ($DAEMON_FILEPATH)"
[ "$(awk -v name="$DAEMON_ASSET" '$2 == name {print $1}' "$tmpdir/SHA256SUMS")" = "$DAEMON_SHA256" ] || {
  echo "Manifest checksum mismatch for $DAEMON_ASSET." >&2; exit 8;
}
download "$RELEASE_URL/$DAEMON_ASSET" "$tmpdir/$DAEMON_ASSET"
verify "$tmpdir/$CLI_ASSET"
verify "$tmpdir/$DAEMON_ASSET"

mkdir -p "$BIN_DIR"
install -m 0755 "$tmpdir/$CLI_ASSET" "$BIN_DIR/byam-bin"
cat >"$BIN_DIR/byam" <<EOF
#!/bin/sh
exec env YAM_URL="\${YAM_URL:-http://$STATION_IP:8099}" "$BIN_DIR/byam-bin" "\$@"
EOF
chmod 0755 "$BIN_DIR/byam"
cat >"$BIN_DIR/yam" <<EOF
#!/bin/sh
exec "$BIN_DIR/byam" "\$@"
EOF
chmod 0755 "$BIN_DIR/yam"
cat >"$tmpdir/betteryamd.init" <<'INIT'
#!/bin/sh
# Managed by BetterYAM
set -eu

root=/data/quazar/betteryam
binary=$root/bin/betteryamd
pidfile=$root/betteryamd.pid

start_daemon() {
    mount -o remount,rw / || {
        echo "BetterYAMd: could not remount / read-write" >&2
        return 1
    }
    mount | grep ' on / ' | grep -q '(rw' || {
        echo "BetterYAMd: root filesystem is not writable" >&2
        return 1
    }
    if [ ! -x "$binary" ]; then
        echo "BetterYAMd: daemon binary is missing: $binary" >&2
        return 1
    fi
    mkdir -p "$root/data"
    if [ -f "$pidfile" ]; then
        pid=$(cat "$pidfile")
        case "$pid" in
            ''|*[!0-9]*) rm -f "$pidfile" ;;
            *)
                if kill -0 "$pid" 2>/dev/null; then
                    return 0
                fi
                rm -f "$pidfile"
                ;;
        esac
    fi
    start-stop-daemon --start --background --make-pidfile \
        --pidfile "$pidfile" --startas "$binary" -- \
        --data-dir "$root/data"
}

stop_daemon() {
    if [ ! -f "$pidfile" ]; then
        return 0
    fi
    pid=$(cat "$pidfile")
    case "$pid" in
        ''|*[!0-9]*)
            echo "BetterYAMd: invalid pid file: $pidfile" >&2
            return 1
            ;;
    esac
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid"
        attempt=0
        while kill -0 "$pid" 2>/dev/null && [ "$attempt" -lt 50 ]; do
            sleep 0.1
            attempt=$((attempt + 1))
        done
        if kill -0 "$pid" 2>/dev/null; then
            echo "BetterYAMd: process $pid did not stop" >&2
            return 1
        fi
    fi
    rm -f "$pidfile"
}

case "${1:-start}" in
    start) start_daemon ;;
    stop) stop_daemon ;;
    restart)
        stop_daemon
        start_daemon
        ;;
    *)
        echo "Usage: $0 {start|stop|restart}" >&2
        exit 2
        ;;
esac
INIT
run_remote_script <<'REMOTE'
set -eu
mount -o remount,rw /
mount | grep ' on / ' | grep -q '(rw' || {
    echo "Root filesystem is not writable after remount." >&2
    exit 1
}
mkdir -p /data/quazar/betteryam/bin /data/quazar/betteryam/data
REMOTE
adb -s "$DEVICE" push "$tmpdir/$DAEMON_ASSET" "$DEVICE_PREFIX/bin/betteryamd.new" </dev/null >/dev/null
adb -s "$DEVICE" push "$tmpdir/betteryamd.init" "$DEVICE_PREFIX/betteryamd.init.new" </dev/null >/dev/null
run_remote_script <<'REMOTE'
set -eu
root=/data/quazar/betteryam
binary=$root/bin/betteryamd
staged=$root/bin/betteryamd.new
backup=$root/bin/betteryamd.previous
staged_init=$root/betteryamd.init.new
init=/etc/init.d/S99zzbetteryam
pidfile=$root/betteryamd.pid

if [ -e "$init" ] && ! grep -q '^# Managed by BetterYAM$' "$init"; then
    echo "Refusing to replace unmanaged init script: $init" >&2
    exit 1
fi
chmod 0755 "$staged" "$staged_init"
if [ -x "$init" ]; then
    "$init" stop
elif [ -f "$pidfile" ]; then
    pid=$(cat "$pidfile")
    case "$pid" in
        ''|*[!0-9]*) echo "Invalid BetterYAMd pid file" >&2; exit 1 ;;
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
            exit 1
        fi
    fi
    rm -f "$pidfile"
fi
if [ -f "$binary" ]; then
    cp "$binary" "$backup"
fi
mv "$staged" "$binary"
mv "$staged_init" "$init"
if "$init" start; then
    sleep 1
    pid=$(cat "$pidfile" 2>/dev/null || true)
    case "$pid" in
        ''|*[!0-9]*) ;;
        *)
            if kill -0 "$pid" 2>/dev/null; then
                exit 0
            fi
            ;;
    esac
fi
echo "BetterYAMd failed to start; restoring previous binary." >&2
"$init" stop || true
if [ -f "$backup" ]; then
    mv "$backup" "$binary"
    "$init" start
fi
exit 1
REMOTE

echo "BetterYAM $VERSION CLI installed in $BIN_DIR/byam (yam alias also available)"
echo "BetterYAMd ($DAEMON_FILEPATH) for $STATION_MODEL deployed to $DEVICE ($DEVICE_PREFIX). Boot startup installed at /etc/init.d/S99zzbetteryam. Open http://$STATION_IP:8099"
