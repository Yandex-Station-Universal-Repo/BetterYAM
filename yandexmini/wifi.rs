//! Wi-Fi of the station through `wpa_cli`, the way the station's own software
//! talks to its wpa_supplicant: the network it is on, networks around, the
//! saved ones; adding and forgetting a network. A network is added with the
//! key derived from its password (PBKDF2, as wpa_passphrase does), so the
//! password is neither kept nor passed through a command line, and saved
//! with `save_config` into the supplicant's own configuration, where the
//! station's software sees it as one of its own.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::process::Command;
use std::{
    fmt,
    path::{Path, PathBuf},
};

pub const IFACE: &str = "wlan0";
/// Where wpa_cli is looked for besides PATH.
const WPA_CLI_PATHS: &[&str] = &[
    "/usr/sbin/wpa_cli",
    "/sbin/wpa_cli",
    "/usr/bin/wpa_cli",
    "/system/bin/wpa_cli",
];
/// Control socket directories, in the order they are tried.
const CTRL_DIRS: &[&str] = &["/var/run/wpa_supplicant", "/run/wpa_supplicant"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub signal_dbm: i32,
    pub secure: bool,
    pub freq: u32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Status {
    /// wpa_supplicant's state (COMPLETED, SCANNING, ...); empty without one
    pub state: String,
    pub ssid: Option<String>,
    pub ip: Option<String>,
    pub freq: Option<u32>,
}

#[derive(Debug)]
pub struct NetworkNotFound(String);

impl fmt::Display for NetworkNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no saved network '{}'", self.0)
    }
}

impl std::error::Error for NetworkNotFound {}

impl Status {
    pub fn connected(&self) -> bool {
        self.state == "COMPLETED" && self.ip.is_some()
    }
}

pub fn find_executable(name: &str, extra: &[&str]) -> Option<PathBuf> {
    let from_path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    from_path
        .into_iter()
        .chain(extra.iter().map(PathBuf::from))
        .find(|p| p.is_file())
}

/// wpa_cli is there and so is the Wi-Fi interface.
pub fn available() -> bool {
    find_executable("wpa_cli", WPA_CLI_PATHS).is_some()
        && Path::new("/sys/class/net").join(IFACE).exists()
}

fn wpa_cli(args: &[&str]) -> Result<String> {
    let exe =
        find_executable("wpa_cli", WPA_CLI_PATHS).context("wpa_cli is not on this station")?;
    let mut cmd = Command::new(exe);
    if let Some(dir) = CTRL_DIRS.iter().find(|d| Path::new(d).is_dir()) {
        cmd.args(["-p", dir]);
    }
    let out = cmd
        .args(["-i", IFACE])
        .args(args)
        .output()
        .context("run wpa_cli")?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() || text.starts_with("Failed to connect") {
        bail!("wpa_supplicant is not answering: {}", text.trim())
    }
    Ok(text)
}

/// A command that answers "OK".
fn wpa_ok(args: &[&str]) -> Result<()> {
    let out = wpa_cli(args)?;
    if out.trim() != "OK" {
        bail!(
            "wpa_cli {}: {}",
            args.first().copied().unwrap_or(""),
            out.trim()
        )
    }
    Ok(())
}

pub fn status() -> Result<Status> {
    Ok(parse_status(&wpa_cli(&["status"])?))
}

fn parse_status(text: &str) -> Status {
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
            .map(str::to_string)
    };
    Status {
        state: field("wpa_state").unwrap_or_default(),
        ssid: field("ssid").map(|s| unescape(&s)),
        ip: field("ip_address"),
        freq: field("freq").and_then(|f| f.parse().ok()),
    }
}

pub fn scan() -> Result<()> {
    // "FAIL-BUSY" while a scan runs already is fine
    let out = wpa_cli(&["scan"])?;
    if out.trim() != "OK" && out.trim() != "FAIL-BUSY" {
        bail!("wpa_cli scan: {}", out.trim())
    }
    Ok(())
}

pub fn scan_results() -> Result<Vec<Network>> {
    Ok(parse_scan_results(&wpa_cli(&["scan_results"])?))
}

/// "bssid / frequency / signal level / flags / ssid" lines; one entry per
/// name, the strongest; hidden networks left out; strongest first.
pub fn parse_scan_results(text: &str) -> Vec<Network> {
    let mut best: Vec<Network> = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 || f[4].is_empty() || f[4].starts_with("\\x00") {
            continue;
        }
        let n = Network {
            ssid: unescape(f[4]),
            freq: f[1].parse().unwrap_or(0),
            signal_dbm: f[2].parse().unwrap_or(-100),
            secure: ["WPA", "WEP", "SAE", "RSN"]
                .iter()
                .any(|k| f[3].contains(k)),
        };
        match best.iter_mut().find(|b| b.ssid == n.ssid) {
            Some(b) if b.signal_dbm >= n.signal_dbm => {}
            Some(b) => *b = n,
            None => best.push(n),
        }
    }
    best.sort_by(|a, b| b.signal_dbm.cmp(&a.signal_dbm));
    best
}

/// The saved networks: (id, ssid).
pub fn saved() -> Result<Vec<(u32, String)>> {
    Ok(parse_list_networks(&wpa_cli(&["list_networks"])?))
}

fn parse_list_networks(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some((f.first()?.parse().ok()?, unescape(f.get(1)?)))
        })
        .collect()
}

pub fn check(ssid: &str, password: &str) -> Result<()> {
    if ssid.is_empty() || ssid.len() > 32 {
        bail!("the network name is 1..32 bytes")
    }
    if !password.is_empty() && !(8..=63).contains(&password.len()) {
        bail!("a password is 8..63 characters (none for an open network)")
    }
    if !password.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        bail!("a password is printable ASCII")
    }
    Ok(())
}

/// The WPA key of a network: PBKDF2-HMAC-SHA1(password, ssid, 4096), 32 bytes.
pub fn psk(ssid: &str, password: &str) -> String {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), ssid.as_bytes(), 4096, &mut key);
    hex(&key)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Add (or replace) a network and save it; `key`: the PSK (64 hex digits),
/// or None for an open network.
pub fn add(ssid: &str, key: Option<&str>) -> Result<()> {
    if ssid.is_empty() || ssid.len() > 32 {
        bail!("the network name is 1..32 bytes")
    }
    for (id, name) in saved()? {
        if name == ssid {
            wpa_ok(&["remove_network", &id.to_string()])?;
        }
    }
    let id = wpa_cli(&["add_network"])?.trim().to_string();
    id.parse::<u32>()
        .with_context(|| format!("wpa_cli add_network: {id}"))?;
    // the name in hex: no quoting, any bytes
    wpa_ok(&["set_network", &id, "ssid", &hex(ssid.as_bytes())])?;
    match key {
        Some(k) => wpa_ok(&["set_network", &id, "psk", k])?,
        None => wpa_ok(&["set_network", &id, "key_mgmt", "NONE"])?,
    }
    // found also when it does not announce its name
    wpa_ok(&["set_network", &id, "scan_ssid", "1"])?;
    wpa_ok(&["enable_network", &id])?;
    wpa_ok(&["save_config"])
        .context("added for now, but wpa_supplicant could not save it (update_config is off?)")
}

/// Ask the station's wpa_supplicant to exit (the setup access point needs
/// the interface).
pub fn terminate() -> Result<()> {
    wpa_ok(&["terminate"])
}

pub fn forget(ssid: &str) -> Result<()> {
    let ids: Vec<u32> = saved()?
        .into_iter()
        .filter(|(_, n)| n == ssid)
        .map(|(id, _)| id)
        .collect();
    if ids.is_empty() {
        return Err(NetworkNotFound(ssid.to_string()).into());
    }
    for id in ids {
        wpa_ok(&["remove_network", &id.to_string()])?;
    }
    wpa_ok(&["save_config"])
}

/// An SSID as wpa_cli prints it (wpa_supplicant's printf_encode: printable
/// ASCII as is, \\ \" \e \n \r \t, any other byte \xNN), back to its bytes,
/// so "\xd0\x94\xd0\xbe\xd0\xbc" is "Дом" again.
pub fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let (byte, len) = match b[i + 1] {
            b'x' => match s
                .get(i + 2..i + 4)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                Some(v) => (v, 4),
                None => (b'\\', 1),
            },
            b'\\' => (b'\\', 2),
            b'"' => (b'"', 2),
            b'e' => (0x1b, 2),
            b'n' => (b'\n', 2),
            b'r' => (b'\r', 2),
            b't' => (b'\t', 2),
            _ => (b'\\', 1),
        };
        out.push(byte);
        i += len;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_back_as_they_are() {
        assert_eq!(unescape("Home_5G"), "Home_5G");
        assert_eq!(unescape("\\xd0\\x94\\xd0\\xbe\\xd0\\xbc"), "Дом");
        assert_eq!(unescape("a\\\\b \\\"q\\\""), "a\\b \"q\"");
        assert_eq!(unescape("50\\%"), "50\\%");
        assert_eq!(unescape("end\\"), "end\\");
    }

    #[test]
    fn psk_matches_wpa_passphrase() {
        // IEEE 802.11i test vector
        assert_eq!(
            psk("IEEE", "password"),
            "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"
        );
    }

    #[test]
    fn scan_results_strongest_per_name() {
        let text = "bssid / frequency / signal level / flags / ssid\n\
            aa:bb:cc:dd:ee:01\t2437\t-70\t[WPA2-PSK-CCMP][ESS]\tHome\n\
            aa:bb:cc:dd:ee:02\t5180\t-50\t[WPA2-PSK-CCMP][ESS]\tHome\n\
            aa:bb:cc:dd:ee:03\t2412\t-60\t[ESS]\tCafe\n\
            aa:bb:cc:dd:ee:04\t2412\t-40\t[WPA2-PSK-CCMP][ESS]\t\n";
        let n = parse_scan_results(text);
        assert_eq!(n.len(), 2);
        assert_eq!(
            (n[0].ssid.as_str(), n[0].signal_dbm, n[0].freq),
            ("Home", -50, 5180)
        );
        assert!(n[0].secure);
        assert_eq!(n[1].ssid, "Cafe");
        assert!(!n[1].secure);
    }

    #[test]
    fn status_and_saved() {
        let s = parse_status("bssid=aa:bb\nfreq=5180\nssid=Home\nid=0\nwpa_state=COMPLETED\nip_address=192.168.1.5\n");
        assert!(s.connected());
        assert_eq!(s.ssid.as_deref(), Some("Home"));
        assert_eq!(s.freq, Some(5180));
        assert!(!parse_status("wpa_state=SCANNING\n").connected());
        let saved = parse_list_networks(
            "network id / ssid / bssid / flags\n0\tHome\tany\t[CURRENT]\n1\t\\xd0\\x94\tany\t\n",
        );
        assert_eq!(saved, vec![(0, "Home".into()), (1, "Д".into())]);
    }

    #[test]
    fn passwords_checked() {
        assert!(check("Home", "").is_ok());
        assert!(check("Home", "12345678").is_ok());
        assert!(check("Home", "short").is_err());
        assert!(check("", "12345678").is_err());
        assert!(check(&"x".repeat(33), "").is_err());
    }
}
