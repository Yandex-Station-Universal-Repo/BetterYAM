#!/bin/sh
# Read-only look at a station's Wi-Fi, for the access-point setup mode
# (raise its own network, pick a Wi-Fi on a web page). Changes nothing.
#
#   adb shell sh < tools/wifi-probe.sh > wifi-probe-$(date +%s).txt
#
# Send the output file to the BetterYAM issue: it says whether the Wi-Fi
# chip and its driver can run an access point, who manages wpa_supplicant,
# and which ports are taken. Passwords and keys are left out; the output
# does show the station's MAC and addresses and the names of the Wi-Fi
# networks around it — cut those before posting it publicly if you like.

section() { echo; echo "=== $*"; }
have() { command -v "$1" >/dev/null 2>&1; }

section system
uname -a
cat /etc/hostname 2>/dev/null
cat /etc/os-release 2>/dev/null
cat /proc/cpuinfo 2>/dev/null | grep -iE "hardware|model name|^processor" | sort -u

section wifi chip
have yandex-wifichipmodel && echo "yandex-wifichipmodel: $(yandex-wifichipmodel 2>&1)"
for d in /sys/class/net/*; do
	n=${d##*/}
	drv=$(basename "$(readlink "$d/device/driver" 2>/dev/null)" 2>/dev/null)
	echo "$n: driver=${drv:-?} phy=$(cat "$d/phy80211/name" 2>/dev/null) mac=$(cat "$d/address" 2>/dev/null) state=$(cat "$d/operstate" 2>/dev/null)"
done
cat /sys/bus/sdio/devices/*/vendor /sys/bus/sdio/devices/*/device 2>/dev/null | tr '\n' ' '; echo
lsmod 2>/dev/null || cat /proc/modules

section wifi modes the driver offers
if have iw; then
	iw list 2>&1 | sed -n '/Supported interface modes/,/^\s*[A-Z]/p'
	iw dev 2>&1
else
	echo "no iw"
fi

section tools present
for b in wpa_supplicant wpa_cli hostapd hostapd_cli dnsmasq udhcpd udhcpc dhcpcd iw ip ifconfig iptables busybox; do
	p=$(command -v $b 2>/dev/null)
	echo "$b: ${p:-absent}"
done
if have busybox; then
	echo "busybox applets with dhcp/dns/http:"
	busybox --list 2>/dev/null | grep -E "dhcp|dns|httpd" | tr '\n' ' '; echo
fi

section wpa_supplicant build
w=$(command -v wpa_supplicant)
if [ -n "$w" ]; then
	"$w" -v 2>&1 | head -2
	# access point support is compiled in when these strings are there
	for s in AP-ENABLED AP-STA-CONNECTED "mode=2"; do
		grep -q "$s" "$w" 2>/dev/null && echo "has '$s'" || echo "no '$s'"
	done
fi

section wpa_supplicant running
ps 2>/dev/null | grep -E "wpa_supplicant|hostapd|dhcp|dnsmasq|udhcp" | grep -v grep
ps -o pid,ppid,args 2>/dev/null | grep -E "wpa_supplicant" | grep -v grep
ls -la /var/run/wpa_supplicant /run/wpa_supplicant 2>/dev/null

section wpa_cli
if have wpa_cli; then
	wpa_cli -i wlan0 status 2>&1 | grep -vE "^(passphrase|psk)="
	echo "--- networks"
	wpa_cli -i wlan0 list_networks 2>&1
	echo "--- scan results"
	wpa_cli -i wlan0 scan_results 2>&1 | head -15
fi

section "wpa_supplicant config, keys hidden"
for f in /bkupgrade/data/wpa_supplicant.conf /etc/wpa_supplicant.conf /data/misc/wifi/wpa_supplicant.conf /etc/wifi.conf; do
	[ -f "$f" ] || continue
	echo "--- $f ($(ls -la "$f" 2>/dev/null))"
	sed -E 's/(psk|password|wep_key[0-9])=.*/\1=<hidden>/' "$f"
done

section wifi init scripts
for f in /etc/init.d/*wifi* /etc/init.d/*net* /etc/init.d/*dhcp*; do
	[ -f "$f" ] && { echo "--- $f"; cat "$f"; }
done

section "who manages the network: quasar"
ps 2>/dev/null | grep -iE "quasar|maind|wifi|network|connman|netd" | grep -v grep

section "ports in use: 53 dns, 67 dhcp, 80 http, 8099 betteryam"
netstat -lnup 2>/dev/null || netstat -lnu 2>/dev/null
netstat -lntp 2>/dev/null || netstat -lnt 2>/dev/null

section addresses and routes
ip addr 2>/dev/null || ifconfig -a
ip route 2>/dev/null || route -n
cat /etc/resolv.conf 2>/dev/null

section done
