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

if [ "$ACTION" = install ]; then
  case "$(uname -s):$(uname -m)" in
    Linux:x86_64|Linux:amd64) CLI_ASSET=yam-x86_64-unknown-linux-musl ;;
    Darwin:arm64|Darwin:aarch64) CLI_ASSET=yam-aarch64-apple-darwin ;;
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
download() {
  url=$1 output=$2
  if command -v curl >/dev/null 2>&1; then
    curl -fL --proto '=https,http' --tlsv1.2 "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$output" "$url"
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
    if [ "$arch" = aarch64 ] || [ "$arch" = arm64 ]; then
      model=$(adb -s "$serial" shell uname -n </dev/null 2>/dev/null | tr -d '\r')
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
  rm -f "$BIN_DIR/yam" "$BIN_DIR/yam-bin"
  adb -s "$DEVICE" shell "if [ -f $DEVICE_PREFIX/betteryamd.pid ] && kill -0 \$(cat $DEVICE_PREFIX/betteryamd.pid) 2>/dev/null; then kill \$(cat $DEVICE_PREFIX/betteryamd.pid); fi; rm -rf $DEVICE_PREFIX" </dev/null
  echo "YAM removed from $BIN_DIR and $DEVICE ($DEVICE_PREFIX)."
  exit 0
fi

command -v python3 >/dev/null 2>&1 || {
  echo 'python3 is required to select the device daemon from manifest.json.' >&2; exit 4;
}

STATION_MODEL=$(adb -s "$DEVICE" shell uname -n </dev/null 2>/dev/null | tr -d '\r' | tr '[:upper:]' '[:lower:]')
MODEL_ID=$(printf '%s' "$STATION_MODEL" | sed 's/[^a-z0-9_-]/-/g; s/--*/-/g; s/^-//; s/-$//')
[ -n "$MODEL_ID" ] || { echo 'Could not determine a valid station model using uname -n.' >&2; exit 7; }

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
[ "$(awk -v name="$DAEMON_ASSET" '$2 == name {print $1}' "$tmpdir/SHA256SUMS")" = "$DAEMON_SHA256" ] || {
  echo "Manifest checksum mismatch for $DAEMON_ASSET." >&2; exit 8;
}
download "$RELEASE_URL/$DAEMON_ASSET" "$tmpdir/$DAEMON_ASSET"
verify "$tmpdir/$CLI_ASSET"
verify "$tmpdir/$DAEMON_ASSET"

mkdir -p "$BIN_DIR"
install -m 0755 "$tmpdir/$CLI_ASSET" "$BIN_DIR/yam-bin"
cat >"$BIN_DIR/yam" <<EOF
#!/bin/sh
exec env YAM_URL="\${YAM_URL:-http://$STATION_IP:8099}" "$BIN_DIR/yam-bin" "\$@"
EOF
chmod 0755 "$BIN_DIR/yam"
adb -s "$DEVICE" shell "mkdir -p $DEVICE_PREFIX/bin $DEVICE_PREFIX/data" </dev/null
adb -s "$DEVICE" push "$tmpdir/$DAEMON_ASSET" "$DEVICE_PREFIX/bin/betteryamd.new" </dev/null >/dev/null
adb -s "$DEVICE" shell "chmod 0755 $DEVICE_PREFIX/bin/betteryamd.new; if [ -f $DEVICE_PREFIX/betteryamd.pid ] && kill -0 \$(cat $DEVICE_PREFIX/betteryamd.pid) 2>/dev/null; then kill \$(cat $DEVICE_PREFIX/betteryamd.pid); fi; mv $DEVICE_PREFIX/bin/betteryamd.new $DEVICE_PREFIX/bin/betteryamd; start-stop-daemon --start --background --make-pidfile --pidfile $DEVICE_PREFIX/betteryamd.pid --startas $DEVICE_PREFIX/bin/betteryamd -- --data-dir $DEVICE_PREFIX/data" </dev/null

echo "BetterYAM $VERSION CLI installed in $BIN_DIR/yam"
echo "BetterYAMd ($DAEMON_FILEPATH) for $STATION_MODEL deployed to $DEVICE ($DEVICE_PREFIX). Open http://$STATION_IP:8099"
