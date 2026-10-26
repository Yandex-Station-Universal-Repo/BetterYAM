//! Wi-Fi setup from a phone: without a network the station raises its own
//! open access point ("BetterYAM-XXXX", 192.168.4.1); a phone that joins it
//! gets an address and a DNS that sends every name here (`captive`), so it
//! shows "sign in to the network" and opens the setup page, where a Wi-Fi
//! network is picked. The station then leaves the access point for it.
//!
//! The way back is always the network it had: every `retry_secs` (not while
//! a phone is on the access point, up to three rounds) it steps out for
//! `try_secs` and looks for its known networks again, so it comes back by
//! itself when the router does. The wpa_supplicant the station's software
//! runs is put back exactly as it was started (its command line is kept),
//! and the access point is given up at once if anything else starts one
//! meanwhile: it never fights the station's own Wi-Fi management.
//!
//! Off unless `[wifi_setup] mode` says "manual" (started from the API or
//! the web page) or "auto" (after `ap_after_secs` without a network), and
//! only where an access point can be run at all: the Mini firmware has no
//! hostapd and its wpa_supplicant is built without AP mode, so a hostapd
//! (static, nl80211) must be put in `<data dir>/bin/hostapd` or on PATH.
//! Until it has been tried on real hardware this is experimental.

use crate::captive::{self, Leases, Subnet};
use crate::wifi::{self, Network};
use anyhow::{bail, Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub const AP_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);
const SUBNET: Subnet = Subnet {
    server: AP_IP,
    first: 10,
    last: 60,
};
const TICK: Duration = Duration::from_secs(5);
/// After a failed start, the access point is not tried again for this long.
const COOLDOWN: Duration = Duration::from_secs(600);
const HOSTAPD_PATHS: &[&str] = &["/usr/sbin/hostapd", "/sbin/hostapd", "/usr/bin/hostapd"];
const PAGE: &str = include_str!("web/wifi.html");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Off,
    Manual,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WifiSetupSettings {
    pub mode: Mode,
    /// empty: "BetterYAM-" and the end of the Wi-Fi MAC
    pub ap_ssid: String,
    pub ap_after_secs: u64,
    pub retry_secs: u64,
    pub try_secs: u64,
    /// 2.4 GHz channel of the access point
    pub channel: u8,
    /// empty: `<data dir>/bin/hostapd`, then PATH
    pub hostapd: String,
}

impl Default for WifiSetupSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            ap_ssid: String::new(),
            ap_after_secs: 180,
            retry_secs: 300,
            try_secs: 60,
            channel: 6,
            hostapd: String::new(),
        }
    }
}

impl WifiSetupSettings {
    pub fn validate(&self) -> Result<()> {
        if !(1..=14).contains(&self.channel) {
            bail!("Wi-Fi setup channel must be 1..14")
        }
        if !self.ap_ssid.is_empty() && self.ap_ssid.len() > 32 {
            bail!("Wi-Fi setup SSID must be at most 32 bytes")
        }
        if self.ap_after_secs > 86_400 {
            bail!("Wi-Fi setup ap_after_secs must be at most 86400 seconds")
        }
        if self.retry_secs == 0 || self.retry_secs > 86_400 {
            bail!("Wi-Fi setup retry_secs must be within 1..86400 seconds")
        }
        if self.try_secs == 0 || self.try_secs > 86_400 {
            bail!("Wi-Fi setup try_secs must be within 1..86400 seconds")
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// on the station's own networks (or trying to be)
    Client,
    /// its own access point is up
    Ap,
    /// stepped out of the access point to look for the known networks
    Try,
}

struct Running {
    hostapd: Child,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
    http: Vec<tokio::task::JoinHandle<()>>,
    leases: Arc<Mutex<Leases>>,
}

struct Inner {
    phase: Phase,
    since: Instant,
    ap_ssid: String,
    /// what was around before the access point came up (it cannot scan)
    around: Vec<Network>,
    saved: Vec<String>,
    /// saved on the access point: added once wpa_supplicant is back
    pending: Option<(String, Option<String>)>,
    /// the last try was for a network saved on the access point
    tried_saved: bool,
    /// ... and it did not connect
    failed: bool,
    error: Option<String>,
    /// start (true) or stop (false) asked for by hand
    request: Option<bool>,
    /// how the station's wpa_supplicant was started, to put it back
    supplicant: Vec<String>,
    running: Option<Running>,
    cooldown_until: Option<Instant>,
    without_network: Duration,
}

pub struct Setup {
    data_dir: PathBuf,
    settings: Arc<tokio::sync::Mutex<crate::Settings>>,
    inner: Mutex<Inner>,
}

impl Setup {
    pub fn new(data_dir: PathBuf, settings: Arc<tokio::sync::Mutex<crate::Settings>>) -> Arc<Self> {
        Arc::new(Self {
            data_dir,
            settings,
            inner: Mutex::new(Inner {
                phase: Phase::Client,
                since: Instant::now(),
                ap_ssid: String::new(),
                around: Vec::new(),
                saved: Vec::new(),
                pending: None,
                tried_saved: false,
                failed: false,
                error: None,
                request: None,
                supplicant: Vec::new(),
                running: None,
                cooldown_until: None,
                without_network: Duration::ZERO,
            }),
        })
    }

    /// The supervisor, in a thread of its own (it waits on processes);
    /// `runtime` serves the setup page while the access point is up.
    pub fn start(self: &Arc<Self>, runtime: tokio::runtime::Handle) {
        let me = self.clone();
        thread::Builder::new()
            .name("wifi-setup".into())
            .spawn(move || loop {
                thread::sleep(TICK);
                let cfg = me.settings.blocking_lock().wifi_setup.clone();
                me.tick(&cfg, &runtime);
            })
            .expect("spawn the Wi-Fi setup thread");
    }

    fn settings(&self) -> WifiSetupSettings {
        self.settings.blocking_lock().wifi_setup.clone()
    }

    fn hostapd(&self, cfg: &WifiSetupSettings) -> Option<PathBuf> {
        if !cfg.hostapd.is_empty() {
            return Some(PathBuf::from(&cfg.hostapd)).filter(|p| p.is_file());
        }
        let own = self.data_dir.join("bin/hostapd");
        if own.is_file() {
            return Some(own);
        }
        wifi::find_executable("hostapd", HOSTAPD_PATHS)
    }

    /// An access point can be raised here: Wi-Fi through wpa_cli, a hostapd.
    pub fn ap_available(&self) -> bool {
        wifi::available() && self.hostapd(&self.settings()).is_some()
    }

    fn tick(self: &Arc<Self>, cfg: &WifiSetupSettings, runtime: &tokio::runtime::Handle) {
        let mut st = self.inner.lock().unwrap();
        let now = Instant::now();
        let since = now.duration_since(st.since);
        match st.phase {
            // off: nothing to watch
            Phase::Client if cfg.mode == Mode::Off => st.without_network = Duration::ZERO,
            Phase::Client => {
                let asked = st.request.take() == Some(true);
                let status = wifi::status();
                let up = match &status {
                    Ok(status) => status.connected(),
                    Err(error) => {
                        st.error = Some(format!("Wi-Fi status unavailable: {error:#}"));
                        tracing::warn!(%error, "wifi setup: could not read Wi-Fi status");
                        if !asked {
                            return;
                        }
                        false
                    }
                };
                if status.is_ok() {
                    st.without_network = if up {
                        Duration::ZERO
                    } else {
                        st.without_network + TICK
                    };
                }
                let cooling = st.cooldown_until.is_some_and(|t| now < t);
                let due = asked
                    || (status.is_ok()
                        && cfg.mode == Mode::Auto
                        && st.without_network >= Duration::from_secs(cfg.ap_after_secs));
                if cfg.mode != Mode::Off && due && !cooling {
                    match self.hostapd(cfg) {
                        Some(hostapd) => {
                            tracing::info!(without_network = ?st.without_network, asked, "wifi setup: raising the access point");
                            self.start_ap(&mut st, cfg, &hostapd, runtime);
                        }
                        None => {
                            st.error = Some(
                                "no hostapd on this station: the access point cannot be raised"
                                    .into(),
                            )
                        }
                    }
                }
            }
            Phase::Ap => {
                let stop_asked = st.request.take() == Some(false) || cfg.mode == Mode::Off;
                if stop_asked {
                    tracing::info!("wifi setup: access point stopped");
                    self.stop_ap(&mut st, true);
                    st.phase = Phase::Client;
                    st.since = now;
                    return;
                }
                // something else started a wpa_supplicant: the station's own
                // software takes Wi-Fi back, and it is not fought over
                if !find_processes("wpa_supplicant").is_empty() {
                    tracing::warn!("wifi setup: a wpa_supplicant was started meanwhile; giving the access point up");
                    self.stop_ap(&mut st, false);
                    st.error = Some(
                        "the station started its own Wi-Fi meanwhile; access point given up".into(),
                    );
                    st.phase = Phase::Client;
                    st.since = now;
                    st.cooldown_until = Some(now + COOLDOWN);
                    return;
                }
                let hostapd_gone = st
                    .running
                    .as_mut()
                    .map(|r| !matches!(r.hostapd.try_wait(), Ok(None)))
                    .unwrap_or(true);
                if hostapd_gone {
                    let log = self.hostapd_log_tail();
                    tracing::error!(%log, "wifi setup: hostapd exited");
                    self.stop_ap(&mut st, true);
                    st.error = Some(format!("hostapd exited: {log}"));
                    st.phase = Phase::Client;
                    st.since = now;
                    st.cooldown_until = Some(now + COOLDOWN);
                    return;
                }
                let phones = st
                    .running
                    .as_ref()
                    .map(|r| phones_present(&r.leases, now))
                    .unwrap_or(0);
                let retry = Duration::from_secs(cfg.retry_secs);
                let timer = since >= retry && (phones == 0 || since >= retry * 3);
                if st.pending.is_some() || timer {
                    let saved = st.pending.take();
                    tracing::info!(
                        saved = saved.is_some(),
                        "wifi setup: leaving the access point to look for the known networks"
                    );
                    self.stop_ap(&mut st, true);
                    st.tried_saved = saved.is_some();
                    if let Some((ssid, key)) = saved {
                        // the supplicant has to answer first
                        let added = wait_for(Duration::from_secs(10), || wifi::status().is_ok())
                            .then(|| wifi::add(&ssid, key.as_deref()))
                            .unwrap_or_else(|| {
                                Err(anyhow::anyhow!("wpa_supplicant did not come back"))
                            });
                        if let Err(e) = added {
                            tracing::error!(error = %e, "wifi setup: could not add the saved network");
                            st.error = Some(format!("could not add '{ssid}': {e}"));
                        }
                    }
                    st.phase = Phase::Try;
                    st.since = now;
                }
            }
            Phase::Try => {
                let connected = match wifi::status() {
                    Ok(status) => status.connected(),
                    Err(error) => {
                        st.error = Some(format!("Wi-Fi status unavailable: {error:#}"));
                        tracing::debug!(%error, "wifi setup: Wi-Fi status is not available during reconnect");
                        false
                    }
                };
                if connected {
                    tracing::info!("wifi setup: network back");
                    st.phase = Phase::Client;
                    st.since = now;
                    st.failed = false;
                    st.error = None;
                    st.without_network = Duration::ZERO;
                } else if since >= Duration::from_secs(cfg.try_secs) {
                    st.failed = st.tried_saved;
                    match (cfg.mode, self.hostapd(cfg)) {
                        (Mode::Off, _) | (_, None) => {
                            st.phase = Phase::Client;
                            st.since = now;
                        }
                        (_, Some(hostapd)) => {
                            tracing::info!(
                                "wifi setup: no known network found, the access point again"
                            );
                            self.start_ap(&mut st, cfg, &hostapd, runtime);
                        }
                    }
                }
            }
        }
    }

    fn run_dir(&self) -> PathBuf {
        self.data_dir.join("run")
    }

    fn hostapd_log_tail(&self) -> String {
        let text = fs::read_to_string(self.run_dir().join("hostapd.log")).unwrap_or_default();
        let lines: Vec<&str> = text.lines().rev().take(5).collect();
        lines.into_iter().rev().collect::<Vec<_>>().join(" | ")
    }

    fn start_ap(
        self: &Arc<Self>,
        st: &mut Inner,
        cfg: &WifiSetupSettings,
        hostapd: &Path,
        runtime: &tokio::runtime::Handle,
    ) {
        match self.try_start_ap(st, cfg, hostapd, runtime) {
            Ok(()) => {
                st.phase = Phase::Ap;
                st.since = Instant::now();
                st.error = None;
                tracing::info!(ssid = %st.ap_ssid, "wifi setup: access point up at {}", AP_IP);
            }
            Err(e) => {
                tracing::error!(error = %e, "wifi setup: the access point did not come up; Wi-Fi put back");
                self.stop_ap(st, true);
                st.error = Some(format!("{e:#}"));
                st.phase = Phase::Client;
                st.since = Instant::now();
                st.cooldown_until = Some(Instant::now() + COOLDOWN);
            }
        }
    }

    fn try_start_ap(
        self: &Arc<Self>,
        st: &mut Inner,
        cfg: &WifiSetupSettings,
        hostapd: &Path,
        runtime: &tokio::runtime::Handle,
    ) -> Result<()> {
        st.saved = match wifi::saved() {
            Ok(networks) => networks.into_iter().map(|(_, ssid)| ssid).collect(),
            Err(error) => {
                tracing::warn!(%error, "wifi setup: could not cache saved network names");
                Vec::new()
            }
        };
        // what is around, for the page: the access point cannot scan
        match wifi::scan() {
            Ok(()) => thread::sleep(Duration::from_secs(4)),
            Err(error) => {
                tracing::warn!(%error, "wifi setup: could not scan before starting the access point")
            }
        }
        match wifi::scan_results() {
            Ok(around) => {
                if !around.is_empty() || st.around.is_empty() {
                    st.around = around;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "wifi setup: could not read scan results before starting the access point")
            }
        }
        // the station's wpa_supplicant steps aside; how it was started is
        // kept to put it back the same way
        let supplicants = find_processes("wpa_supplicant");
        st.supplicant = supplicants
            .iter()
            .find(|(_, args)| args.iter().any(|a| a.contains(wifi::IFACE)))
            .or(supplicants.first())
            .map(|(_, args)| args.clone())
            .unwrap_or_default();
        if !supplicants.is_empty() {
            let _ = wifi::terminate();
            if !wait_for(Duration::from_secs(5), || {
                find_processes("wpa_supplicant").is_empty()
            }) {
                for (pid, _) in find_processes("wpa_supplicant") {
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                }
                if !wait_for(Duration::from_secs(5), || {
                    find_processes("wpa_supplicant").is_empty()
                }) {
                    bail!("the station's wpa_supplicant does not stop");
                }
            }
        }
        st.ap_ssid = if cfg.ap_ssid.is_empty() {
            default_ssid()
        } else {
            cfg.ap_ssid.clone()
        };
        let run = self.run_dir();
        fs::create_dir_all(&run).context("create the run directory")?;
        let conf = run.join("hostapd.conf");
        fs::write(&conf, hostapd_conf(&st.ap_ssid, cfg.channel)).context("write hostapd.conf")?;
        let log = fs::File::create(run.join("hostapd.log")).context("create hostapd.log")?;
        let mut child = Command::new(hostapd)
            .arg(&conf)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .with_context(|| format!("start {}", hostapd.display()))?;
        thread::sleep(Duration::from_secs(2));
        if !matches!(child.try_wait(), Ok(None)) {
            bail!("hostapd exited at once: {}", self.hostapd_log_tail());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let leases = Arc::new(Mutex::new(Leases::default()));
        let mut running = Running {
            hostapd: child,
            stop: stop.clone(),
            threads: Vec::new(),
            http: Vec::new(),
            leases: leases.clone(),
        };
        let started = (|| -> Result<()> {
            set_address(Some(AP_IP))?;
            running
                .threads
                .push(dhcp_server(stop.clone(), leases.clone())?);
            match dns_server(stop.clone()) {
                Ok(t) => running.threads.push(t),
                // phones then do not open the page by themselves, but
                // http://192.168.4.1 still works
                Err(e) => tracing::warn!(error = %e, "wifi setup: no DNS on the access point"),
            }
            for port in [80, 8099] {
                match portal(runtime, port, self.clone()) {
                    Ok(h) => running.http.push(h),
                    Err(e) => {
                        tracing::warn!(port, error = %e, "wifi setup: setup page not served on this port")
                    }
                }
            }
            if running.http.is_empty() {
                bail!("the setup page could not be served on port 80 or 8099");
            }
            Ok(())
        })();
        st.running = Some(running);
        started
    }

    /// Everything of the access point down; with `restore`, the station's
    /// wpa_supplicant started again as it was.
    fn stop_ap(&self, st: &mut Inner, restore: bool) {
        if let Some(mut r) = st.running.take() {
            r.stop.store(true, Ordering::SeqCst);
            for h in r.http {
                h.abort();
            }
            for t in r.threads {
                let _ = t.join();
            }
            let _ = r.hostapd.kill();
            let _ = r.hostapd.wait();
        }
        if let Err(error) = set_address(None) {
            tracing::warn!(%error, "wifi setup: could not remove the access point address");
        }
        if restore && !st.supplicant.is_empty() && find_processes("wpa_supplicant").is_empty() {
            let mut args = st.supplicant.clone();
            // in the background whatever it was, never a child that dies with us
            if !args.iter().any(|a| a == "-B") {
                args.push("-B".into());
            }
            match Command::new(&args[0])
                .args(&args[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(s) if s.success() => {
                    tracing::info!("wifi setup: the station's wpa_supplicant is back")
                }
                Ok(s) => {
                    tracing::error!(status = %s, "wifi setup: the station's wpa_supplicant did not start again")
                }
                Err(e) => {
                    tracing::error!(error = %e, "wifi setup: the station's wpa_supplicant did not start again")
                }
            }
        }
    }

    /// Ask for the access point by hand (mode "manual" or "auto").
    pub fn request(&self, on: bool) -> Result<()> {
        let cfg = self.settings();
        if cfg.mode == Mode::Off {
            bail!("Wi-Fi setup mode is off ([wifi_setup] mode = \"manual\" or \"auto\")")
        }
        if on && self.hostapd(&cfg).is_none() {
            bail!("no hostapd on this station: the access point cannot be raised")
        }
        let mut st = self.inner.lock().unwrap();
        st.request = Some(on);
        st.cooldown_until = None;
        Ok(())
    }

    /// The network as the page shows it.
    pub fn status(&self) -> Result<Value> {
        let cfg = self.settings();
        cfg.validate()?;
        let mut st = self.inner.lock().unwrap();
        let on_ap = st.phase == Phase::Ap;
        let (current, saved, around) = if on_ap {
            (None, st.saved.clone(), st.around.clone())
        } else {
            if !wifi::available() {
                bail!("Wi-Fi is not available: wpa_cli or interface wlan0 is missing")
            }
            let status = wifi::status().context("read Wi-Fi status")?;
            let current = status.connected().then_some(status);
            let saved: Vec<String> = wifi::saved()
                .context("read saved Wi-Fi networks")?
                .into_iter()
                .map(|(_, n)| n)
                .collect();
            st.saved.clone_from(&saved);
            (
                current,
                saved,
                wifi::scan_results().context("read Wi-Fi scan results")?,
            )
        };
        let around: Vec<Value> = around
            .iter()
            .map(|n| json!({"ssid": n.ssid, "signal_dbm": n.signal_dbm, "secure": n.secure, "freq": n.freq, "saved": saved.contains(&n.ssid)}))
            .collect();
        Ok(json!({
            "available": wifi::available(),
            "current": current,
            "saved": saved,
            "around": around,
            "setup": {
                "mode": cfg.mode,
                "ap_available": wifi::available() && self.hostapd(&cfg).is_some(),
                "phase": st.phase,
                "ap_ssid": if on_ap { Some(st.ap_ssid.clone()) } else { None },
                "ap_ip": AP_IP.to_string(),
                "pending": st.pending.as_ref().map(|(s, _)| s.clone()),
                "failed": st.failed,
                "error": st.error,
            }
        }))
    }

    /// A network from the page: added now, or on the access point once the
    /// station is back on its own Wi-Fi. True: the station leaves the
    /// access point for it (the phone loses it).
    pub fn add(&self, ssid: &str, password: &str) -> Result<bool> {
        wifi::check(ssid, password)?;
        let key = (!password.is_empty()).then(|| wifi::psk(ssid, password));
        let mut st = self.inner.lock().unwrap();
        if st.phase == Phase::Ap {
            st.pending = Some((ssid.to_string(), key));
            st.failed = false;
            return Ok(true);
        }
        drop(st);
        wifi::add(ssid, key.as_deref())?;
        Ok(false)
    }

    pub fn forget(&self, ssid: &str) -> Result<()> {
        if self.inner.lock().unwrap().phase == Phase::Ap {
            bail!("not while the station is on its own access point")
        }
        wifi::forget(ssid)
    }

    pub fn scan(&self) -> Result<bool> {
        if self.inner.lock().unwrap().phase == Phase::Ap {
            // the access point cannot scan: its list is the one from before
            return Ok(false);
        }
        wifi::scan()?;
        Ok(true)
    }
}

fn hostapd_conf(ssid: &str, channel: u8) -> String {
    let hex: String = ssid.bytes().map(|b| format!("{b:02x}")).collect();
    format!(
        "interface={}\ndriver=nl80211\nssid2={hex}\nutf8_ssid=1\nhw_mode=g\nchannel={channel}\nauth_algs=1\nignore_broadcast_ssid=0\n",
        wifi::IFACE
    )
}

fn default_ssid() -> String {
    let mac =
        fs::read_to_string(format!("/sys/class/net/{}/address", wifi::IFACE)).unwrap_or_default();
    let tail: String = mac
        .trim()
        .split(':')
        .skip(4)
        .collect::<String>()
        .to_uppercase();
    if tail.is_empty() {
        "BetterYAM".into()
    } else {
        format!("BetterYAM-{tail}")
    }
}

fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Processes with this executable name: pid and command line.
fn find_processes(name: &str) -> Vec<(i32, Vec<String>)> {
    let Ok(dir) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| {
            let pid: i32 = e.file_name().to_str()?.parse().ok()?;
            let raw = fs::read(e.path().join("cmdline")).ok()?;
            let args: Vec<String> = raw
                .split(|b| *b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            let exe = Path::new(args.first()?).file_name()?.to_str()?.to_string();
            (exe == name).then_some((pid, args))
        })
        .collect()
}

/// Phones on the access point: an address asked for lately, or one the
/// station talks to now (the kernel's neighbour table).
fn phones_present(leases: &Mutex<Leases>, now: Instant) -> usize {
    let asked = leases.lock().unwrap().recent(now, Duration::from_secs(600));
    let talking = fs::read_to_string("/proc/net/arp")
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            f.len() >= 6 && f[5] == wifi::IFACE && f[2] == "0x2" && f[0].starts_with("192.168.4.")
        })
        .count();
    asked.max(talking)
}

/// The access point's address on the interface (`None`: removed).
fn set_address(ip: Option<Ipv4Addr>) -> Result<()> {
    let ok = |cmd: &mut Command| -> Result<bool> {
        let program = cmd.get_program().to_string_lossy().into_owned();
        match cmd.stdout(Stdio::null()).stderr(Stdio::null()).status() {
            Ok(status) => Ok(status.success()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).with_context(|| format!("run {program}")),
        }
    };
    let iface = wifi::IFACE;
    match ip {
        Some(ip) => {
            let cidr = format!("{ip}/24");
            let with_ip = ok(Command::new("ip").args(["addr", "flush", "dev", iface]))?
                && ok(Command::new("ip").args(["addr", "add", &cidr, "dev", iface]))?
                && ok(Command::new("ip").args(["link", "set", iface, "up"]))?;
            if !with_ip
                && !ok(Command::new("ifconfig").args([
                    iface,
                    &ip.to_string(),
                    "netmask",
                    "255.255.255.0",
                    "up",
                ]))?
            {
                bail!("could not give {iface} the address {ip}");
            }
        }
        None => {
            if !ok(Command::new("ip").args(["addr", "flush", "dev", iface]))?
                && !ok(Command::new("ifconfig").args([iface, "0.0.0.0"]))?
            {
                bail!("could not remove the access point address from {iface}");
            }
        }
    }
    Ok(())
}

fn udp_socket(addr: SocketAddr, device: Option<&str>, broadcast: bool) -> Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    s.set_broadcast(broadcast)?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(d) = device {
        s.bind_device(Some(d.as_bytes()))
            .with_context(|| format!("bind to {d}"))?;
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let _ = device;
    s.bind(&addr.into())
        .with_context(|| format!("bind {addr}"))?;
    let s: UdpSocket = s.into();
    s.set_read_timeout(Some(Duration::from_secs(1)))?;
    Ok(s)
}

fn dhcp_server(
    stop: Arc<AtomicBool>,
    leases: Arc<Mutex<Leases>>,
) -> Result<thread::JoinHandle<()>> {
    let socket = udp_socket(
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 67)),
        Some(wifi::IFACE),
        true,
    )
    .context("DHCP server")?;
    Ok(thread::spawn(move || {
        let mut buf = [0u8; 1500];
        while !stop.load(Ordering::SeqCst) {
            let Ok((n, _)) = socket.recv_from(&mut buf) else {
                continue;
            };
            let reply = captive::dhcp_reply(
                &SUBNET,
                &mut leases.lock().unwrap(),
                &buf[..n],
                Instant::now(),
            );
            if let Some(reply) = reply {
                let _ = socket.send_to(&reply, SocketAddr::from((Ipv4Addr::BROADCAST, 68)));
            }
        }
    }))
}

fn dns_server(stop: Arc<AtomicBool>) -> Result<thread::JoinHandle<()>> {
    let socket = udp_socket(SocketAddr::from((AP_IP, 53)), None, false).context("DNS server")?;
    Ok(thread::spawn(move || {
        let mut buf = [0u8; 1500];
        while !stop.load(Ordering::SeqCst) {
            let Ok((n, from)) = socket.recv_from(&mut buf) else {
                continue;
            };
            if let Some(reply) = captive::dns_reply(&buf[..n], AP_IP) {
                let _ = socket.send_to(&reply, from);
            }
        }
    }))
}

/// The setup page and its API on the access point's address. Any other
/// path (a phone's connectivity probe: /generate_204, /hotspot-detect.html,
/// ...) is sent to the page.
pub fn portal_router(setup: Arc<Setup>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/api/wifi", get(portal_status))
        .route("/api/wifi/scan", post(portal_scan))
        .route("/api/wifi/networks", get(portal_networks).post(portal_add))
        .route("/api/wifi/networks/:ssid", delete(portal_forget))
        .route(
            "/api/wifi/setup",
            get(portal_setup_status).post(portal_setup),
        )
        .fallback(|| async { Redirect::temporary(&format!("http://{AP_IP}/")) })
        .with_state(setup)
}

fn portal(
    runtime: &tokio::runtime::Handle,
    port: u16,
    setup: Arc<Setup>,
) -> Result<tokio::task::JoinHandle<()>> {
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind((AP_IP, port)))
        .with_context(|| format!("listen on {AP_IP}:{port}"))?;
    let router = portal_router(setup);
    Ok(runtime.spawn(async move {
        let _ = axum::serve(listener, router).await;
    }))
}

#[derive(Deserialize)]
pub struct NetworkRequest {
    pub ssid: String,
    #[serde(default)]
    pub password: String,
}

fn error(status: StatusCode, e: anyhow::Error) -> Response {
    (status, Json(json!({"error": format!("{e:#}")}))).into_response()
}

async fn portal_status(State(setup): State<Arc<Setup>>) -> Response {
    match tokio::task::spawn_blocking(move || setup.status()).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_GATEWAY, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.into()),
    }
}

async fn portal_networks(State(setup): State<Arc<Setup>>) -> Response {
    match tokio::task::spawn_blocking(move || setup.status()).await {
        Ok(Ok(status)) => Json(json!({
            "saved": status["saved"],
            "around": status["around"]
        }))
        .into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_GATEWAY, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.into()),
    }
}

async fn portal_setup_status(State(setup): State<Arc<Setup>>) -> Response {
    match tokio::task::spawn_blocking(move || setup.status()).await {
        Ok(Ok(status)) => Json(status["setup"].clone()).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_GATEWAY, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.into()),
    }
}

async fn portal_add(State(setup): State<Arc<Setup>>, Json(req): Json<NetworkRequest>) -> Response {
    if let Err(error) = wifi::check(&req.ssid, &req.password) {
        return self::error(StatusCode::BAD_REQUEST, error);
    }
    match tokio::task::spawn_blocking(move || setup.add(&req.ssid, &req.password)).await {
        Ok(Ok(leaving)) => Json(json!({"leaving": leaving})).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_GATEWAY, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.into()),
    }
}

async fn portal_scan(State(setup): State<Arc<Setup>>) -> Response {
    match tokio::task::spawn_blocking(move || setup.scan()).await {
        Ok(Ok(started)) => Json(json!({"started": started})).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_GATEWAY, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.into()),
    }
}

async fn portal_forget(
    State(setup): State<Arc<Setup>>,
    axum::extract::Path(ssid): axum::extract::Path<String>,
) -> Response {
    match tokio::task::spawn_blocking(move || setup.forget(&ssid)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(e)) if e.downcast_ref::<wifi::NetworkNotFound>().is_some() => {
            error(StatusCode::NOT_FOUND, e)
        }
        Ok(Err(e)) => error(StatusCode::BAD_GATEWAY, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.into()),
    }
}

#[derive(Deserialize)]
struct SetupRequest {
    on: bool,
}

async fn portal_setup(State(setup): State<Arc<Setup>>, Json(req): Json<SetupRequest>) -> Response {
    match tokio::task::spawn_blocking(move || setup.request(req.on)).await {
        Ok(Ok(())) => StatusCode::ACCEPTED.into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostapd_conf_names_the_network_in_hex() {
        let c = hostapd_conf("Дом", 6);
        assert!(c.contains("ssid2=d094d0bed0bc\n"));
        assert!(c.contains("interface=wlan0\n"));
        assert!(c.contains("channel=6\n"));
        assert!(!c.contains("wpa="), "the setup network is open");
    }

    #[test]
    fn settings_default_off() {
        let s: WifiSetupSettings = toml::from_str("").unwrap();
        assert_eq!(s.mode, Mode::Off);
        assert_eq!(s.ap_after_secs, 180);
        let s: WifiSetupSettings = toml::from_str("mode = \"auto\"\nretry_secs = 120\n").unwrap();
        assert_eq!((s.mode, s.retry_secs, s.try_secs), (Mode::Auto, 120, 60));
    }

    #[test]
    fn settings_validate_channel_ssid_and_intervals() {
        let mut settings = WifiSetupSettings::default();
        assert!(settings.validate().is_ok());

        settings.channel = 0;
        assert!(settings.validate().is_err());
        settings.channel = 6;
        settings.ap_ssid = "a".repeat(33);
        assert!(settings.validate().is_err());
        settings.ap_ssid.clear();
        settings.retry_secs = 0;
        assert!(settings.validate().is_err());
    }
}
