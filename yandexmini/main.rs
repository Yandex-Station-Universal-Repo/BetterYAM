use anyhow::{anyhow, bail, Context, Result};
mod audio;
mod bluetooth;
mod captive;
mod input;
mod ipc;
mod led;
mod sensors;
mod volume;
mod wifi;
mod wifi_setup;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{delete, get, post, put},
    Json, Router,
};
use clap::Parser;
use flate2::read::GzDecoder;
use led::{DeviceInfo, Frame, Led, LedSettings};
use mlua::{Lua, LuaOptions, LuaSerdeExt, StdLib};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::CStr,
    fs,
    io::{Cursor, Read},
    net::{Ipv4Addr, SocketAddr},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Mutex, task};
use tower_http::trace::TraceLayer;
use url::Url;
use uuid::Uuid;

#[derive(Parser)]
struct Args {
    #[arg(long, value_name = "IP:PORT")]
    listen: Option<SocketAddr>,
    #[arg(long, default_value = "/opt/betteryam")]
    data_dir: PathBuf,
    #[arg(long)]
    model: Option<String>,
}
fn validate_ipc_policy(m: &Manifest) -> Result<()> {
    let has_raw_ipc = m
        .permissions
        .iter()
        .any(|permission| matches!(permission.as_str(), "ipc.read" | "ipc.control"));
    if m.ipc.services.is_empty() {
        if has_raw_ipc {
            bail!("ipc.read and ipc.control require a non-empty [ipc].services list")
        }
        return Ok(());
    }
    if !has_raw_ipc {
        bail!("[ipc].services requires ipc.read or ipc.control permission")
    }
    if m.ipc.services.len() > 16 {
        bail!("[ipc].services lists more than 16 services")
    }
    let mut unique = HashSet::new();
    for service in &m.ipc.services {
        ipc::validate_service(service)?;
        if !unique.insert(service) {
            bail!("[ipc].services contains a duplicate service")
        }
    }
    Ok(())
}
async fn logo_set(State(a): State<App>, Json(frame): Json<Frame>) -> Result<StatusCode, ApiError> {
    a.led.draw(frame.colors).await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn logo_off(State(a): State<App>) -> Result<StatusCode, ApiError> {
    a.led.off().await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    led: LedSettings,
    #[serde(default)]
    web: WebSettings,
    #[serde(default)]
    allow_plugin_binaries: bool,
    /// Wi-Fi setup from a phone through the station's own access point
    #[serde(default)]
    wifi_setup: wifi_setup::WifiSetupSettings,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebSettings {
    #[serde(default = "default_true")]
    ui: bool,
    #[serde(default = "default_true")]
    api: bool,
}
fn default_true() -> bool {
    true
}
impl Default for WebSettings {
    fn default() -> Self {
        Self {
            ui: true,
            api: true,
        }
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            led: LedSettings::default(),
            web: WebSettings::default(),
            allow_plugin_binaries: false,
            wifi_setup: wifi_setup::WifiSetupSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    entry: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    settings: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    network: NetworkPolicy,
    #[serde(default)]
    input: InputPolicy,
    #[serde(default)]
    ipc: IpcPolicy,
    #[serde(default)]
    compatibility: Compatibility,
    #[serde(default)]
    launch: Launch,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct NetworkPolicy {
    /// Exact host names or a DNS suffix prefixed with `*.`.
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    allow_insecure_http: bool,
}
/// Input devices a package may read through `alice.input.next_event`, by kernel device name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct InputPolicy {
    #[serde(default)]
    devices: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct IpcPolicy {
    #[serde(default)]
    services: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Launch {
    #[serde(default)]
    kind: LaunchKind,
    #[serde(default)]
    mode: LaunchMode,
    #[serde(default)]
    path: String,
    #[serde(default)]
    args: Vec<String>,
    /// Runs `path` through this interpreter; only `sh` is accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interpreter: Option<String>,
    /// Package-relative directories prepended to `LD_LIBRARY_PATH`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    library_paths: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum LaunchKind {
    #[default]
    Lua,
    Binary,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum LaunchMode {
    #[default]
    Oneshot,
    Service,
}
impl Launch {
    fn path<'a>(&'a self, manifest: &'a Manifest) -> &'a str {
        if self.path.is_empty() {
            &manifest.entry
        } else {
            &self.path
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Compatibility {
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    features: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct DeviceProfile {
    id: String,
    model: String,
    architecture: String,
    led_count: usize,
    features: Vec<String>,
    #[serde(default)]
    capabilities: HashMap<String, bool>,
    source: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct SensorInfo {
    id: String,
    kind: String,
    unit: String,
    value: Option<f64>,
}
#[derive(Debug, Serialize, Deserialize)]
struct ClockInfo {
    available: bool,
    segments: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
struct ClockRequest {
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    brightness: Option<u8>,
    #[serde(default)]
    segments: Option<HashMap<String, u8>>,
    #[serde(default)]
    off: bool,
}
#[derive(Debug, Deserialize)]
struct VolumeRequest {
    percent: u8,
}
#[derive(Debug, Deserialize)]
struct BluetoothPowerRequest {
    on: bool,
}
#[derive(Debug, Deserialize)]
struct WifiSetupRequest {
    on: bool,
}
#[derive(Debug, Deserialize)]
struct RuntimeInputRequest {
    devices: Vec<String>,
    timeout_ms: u64,
}
#[derive(Debug, Clone, Serialize)]
struct PluginInfo {
    manifest: Manifest,
    /// Absolute package directory on the speaker, used by `yam tui`.
    directory: PathBuf,
    enabled: bool,
    running: bool,
    compatible: bool,
    incompatibility: Option<String>,
}
#[derive(Clone)]
struct App {
    root: PathBuf,
    settings: Arc<Mutex<Settings>>,
    device: Arc<Mutex<DeviceProfile>>,
    led: Arc<Led>,
    plugins: Arc<Mutex<PluginManager>>,
    runtime_tokens: Arc<Mutex<HashMap<String, String>>>,
    runtime_api_url: String,
    wifi: Arc<wifi_setup::Setup>,
}
#[derive(Deserialize)]
struct Invocation {
    #[serde(default)]
    args: Vec<String>,
}
struct PluginManager {
    root: PathBuf,
    commands_dir: PathBuf,
    profile_file: PathBuf,
    enabled: HashSet<String>,
    running: HashMap<String, Arc<AtomicBool>>,
}
impl PluginManager {
    fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(root)?;
        let commands_dir = root
            .parent()
            .context("plugin directory has no data directory")?
            .join("bin");
        let enabled = match fs::read_to_string(root.join("enabled.json")) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|error| {
                tracing::warn!(%error, "enabled.json is corrupt; all packages start disabled");
                HashSet::new()
            }),
            Err(_) => HashSet::new(),
        };
        let mut manager = Self {
            root,
            commands_dir,
            profile_file: PathBuf::from("/etc/profile.d/betteryam.sh"),
            enabled,
            running: HashMap::new(),
        };
        manager.sync_commands()?;
        Ok(manager)
    }
    #[cfg(test)]
    fn with_paths(root: PathBuf, commands_dir: PathBuf, profile_file: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let mut manager = Self {
            root: fs::canonicalize(root)?,
            commands_dir,
            profile_file,
            enabled: HashSet::new(),
            running: HashMap::new(),
        };
        manager.sync_commands()?;
        Ok(manager)
    }
    fn sync_commands(&mut self) -> Result<()> {
        fs::create_dir_all(&self.commands_dir)?;
        let mut commands = HashSet::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let manifest_path = entry.path().join("plugin.toml");
            let raw = match fs::read_to_string(&manifest_path) {
                Ok(raw) => raw,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    tracing::warn!(path = %manifest_path.display(), %error, "cannot read package manifest while syncing shell commands");
                    continue;
                }
            };
            let manifest: Manifest = match toml::from_str(&raw) {
                Ok(manifest) => manifest,
                Err(error) => {
                    tracing::warn!(path = %manifest_path.display(), %error, "cannot parse package manifest while syncing shell commands");
                    continue;
                }
            };
            if manifest.launch.kind != LaunchKind::Binary
                || !manifest
                    .permissions
                    .iter()
                    .any(|permission| permission == "exec")
            {
                continue;
            }
            validate_id(&manifest.id)?;
            let package_root = self.root.join(&manifest.id);
            let executable = package_root.join(manifest.launch.path(&manifest));
            if !executable.is_file() {
                tracing::warn!(
                    package = %manifest.id,
                    path = %executable.display(),
                    "binary package launch path is missing; shell command was not created"
                );
                continue;
            }
            let wrapper = self.commands_dir.join(&manifest.id);
            match fs::symlink_metadata(&wrapper) {
                Ok(metadata) => {
                    let mut marker = vec![0; COMMAND_WRAPPER_MARKER.len()];
                    let managed = metadata.file_type().is_file()
                        && fs::File::open(&wrapper)
                            .and_then(|mut file| file.read_exact(&mut marker))
                            .is_ok()
                        && marker == COMMAND_WRAPPER_MARKER.as_bytes();
                    if !managed {
                        bail!(
                            "refusing to overwrite unmanaged shell command {}",
                            wrapper.display()
                        )
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            write_atomic(
                &wrapper,
                command_wrapper(&manifest, &package_root)?.as_bytes(),
            )?;
            set_executable(&wrapper)?;
            commands.insert(manifest.id);
        }

        for entry in fs::read_dir(&self.commands_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let mut marker = vec![0; COMMAND_WRAPPER_MARKER.len()];
            let managed = fs::File::open(&path)
                .and_then(|mut file| file.read_exact(&mut marker))
                .is_ok()
                && marker == COMMAND_WRAPPER_MARKER.as_bytes();
            if managed
                && !commands.contains(
                    &path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_owned(),
                )
            {
                fs::remove_file(path)?;
            }
        }

        if commands.is_empty() {
            match fs::read_to_string(&self.profile_file) {
                Ok(contents) if contents.starts_with(PROFILE_MARKER) => {
                    fs::remove_file(&self.profile_file)?;
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            let profile_dir = self
                .profile_file
                .parent()
                .context("shell profile path has no parent directory")?;
            fs::create_dir_all(profile_dir)?;
            if self.profile_file.exists()
                && !fs::read_to_string(&self.profile_file)?.starts_with(PROFILE_MARKER)
            {
                bail!(
                    "refusing to overwrite unmanaged shell profile {}",
                    self.profile_file.display()
                )
            }
            let path = shell_quote(&self.commands_dir.to_string_lossy());
            let profile = format!(
                "{PROFILE_MARKER}\nBETTERYAM_COMMANDS_DIR={path}\n\
                 case \":${{PATH-}}:\" in\n\
                   *:\"$BETTERYAM_COMMANDS_DIR\":*) ;;\n\
                   *) PATH=\"${{PATH:+$PATH:}}$BETTERYAM_COMMANDS_DIR\" ;;\n\
                 esac\n\
                 export PATH\n\
                 unset BETTERYAM_COMMANDS_DIR\n"
            );
            write_atomic(&self.profile_file, profile.as_bytes())?;
        }
        Ok(())
    }
    fn save(&self) -> Result<()> {
        write_atomic(
            &self.root.join("enabled.json"),
            &serde_json::to_vec_pretty(&self.enabled)?,
        )
    }
    fn list(&self, device: &DeviceProfile) -> Result<Vec<PluginInfo>> {
        let mut out = vec![];
        for e in fs::read_dir(&self.root)? {
            let e = e?;
            if !e.file_type()?.is_dir() {
                continue;
            };
            let p = e.path().join("plugin.toml");
            if let Ok(x) = fs::read_to_string(p) {
                if let Ok(manifest) = toml::from_str::<Manifest>(&x) {
                    let reason = compatibility_error(&manifest, device);
                    out.push(PluginInfo {
                        directory: e.path(),
                        enabled: self.enabled.contains(&manifest.id),
                        running: self.running.contains_key(&manifest.id),
                        compatible: reason.is_none(),
                        incompatibility: reason,
                        manifest,
                    })
                }
            }
        }
        Ok(out)
    }
    fn manifest(&self, id: &str) -> Result<Manifest> {
        validate_id(id)?;
        Ok(toml::from_str(&fs::read_to_string(
            self.root.join(id).join("plugin.toml"),
        )?)?)
    }
    fn install(&mut self, bytes: &[u8]) -> Result<Manifest> {
        let stage = self.root.join(format!(".install-{}", Uuid::new_v4()));
        let budget = unpack_budget(&self.root)?;
        fs::create_dir(&stage)?;
        let result = self.install_staged(bytes, &stage, budget);
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        let manifest = result?;
        self.sync_commands()?;
        Ok(manifest)
    }
    fn install_staged(&self, bytes: &[u8], stage: &Path, budget: u64) -> Result<Manifest> {
        if bytes.starts_with(b"PK\x03\x04") {
            extract_zip_yap(bytes, stage, budget)?
        } else if bytes.starts_with(&[0x1f, 0x8b]) {
            extract_tar_gz_yap(bytes, stage, budget)?
        } else {
            bail!(".yap must be a ZIP or gzip-compressed TAR archive")
        }
        let manifest_text = fs::read_to_string(stage.join("plugin.toml"))
            .context(".yap must contain root plugin.toml")?;
        let m: Manifest = toml::from_str(&manifest_text)?;
        validate_id(&m.id)?;
        safe_relative(&m.entry)?;
        safe_relative(m.launch.path(&m))?;
        for directory in &m.launch.library_paths {
            safe_relative(directory)?;
        }
        if m.launch.interpreter.as_deref().is_some_and(|x| x != "sh") {
            bail!("launch.interpreter supports only \"sh\"")
        }
        if m.launch.kind == LaunchKind::Binary && !m.permissions.iter().any(|p| p == "exec") {
            bail!("binary packages require the exec permission")
        }
        validate_input_policy(&m)?;
        validate_ipc_policy(&m)?;
        if m.launch.kind == LaunchKind::Binary {
            let executable = stage.join(m.launch.path(&m));
            if !executable.is_file() {
                bail!("binary launch path does not name a file")
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for entry in walkdir::WalkDir::new(stage) {
                    let entry = entry?;
                    if entry.file_type().is_file() {
                        let mut permissions = fs::metadata(entry.path())?.permissions();
                        permissions.set_mode(permissions.mode() | 0o111);
                        fs::set_permissions(entry.path(), permissions)?;
                    }
                }
            }
        }
        let target = self.root.join(&m.id);
        if target.exists() {
            fs::remove_dir_all(&target)?
        };
        fs::rename(stage, target)?;
        Ok(m)
    }
    fn remove(&mut self, id: &str) -> Result<()> {
        validate_id(id)?;
        let p = self.root.join(id);
        if !p.exists() {
            bail!("plugin not found")
        };
        if let Some(token) = self.running.remove(id) {
            token.store(true, Ordering::Relaxed);
        }
        fs::remove_dir_all(p)?;
        self.enabled.remove(id);
        self.save()?;
        self.sync_commands()
    }
}

const COMMAND_WRAPPER_MARKER: &str = "# Generated by BetterYAM; do not edit.\n";
const PROFILE_MARKER: &str = "# Managed by BetterYAM; do not edit.\n";

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn command_wrapper(manifest: &Manifest, root: &Path) -> Result<String> {
    let executable = manifest.launch.path(manifest);
    safe_relative(executable)?;
    let (interpreter, program) = launch_program(manifest, root, executable);
    let libraries = binary_library_directories(manifest, root)
        .iter()
        .map(|directory| shell_quote(&directory.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(":");
    let mut wrapper = String::from(COMMAND_WRAPPER_MARKER);
    wrapper.push_str("set -eu\n");
    wrapper.push_str(&format!("cd {}\n", shell_quote(&root.to_string_lossy())));
    if !libraries.is_empty() {
        wrapper.push_str(&format!(
            "if [ -n \"${{LD_LIBRARY_PATH:-}}\" ]; then\n\
               export LD_LIBRARY_PATH={libraries}:\"$LD_LIBRARY_PATH\"\n\
             else\n\
               export LD_LIBRARY_PATH={libraries}\n\
             fi\n"
        ));
    }
    wrapper.push_str("exec ");
    if interpreter.is_some() {
        wrapper.push_str("sh ");
    }
    wrapper.push_str(&shell_quote(&program.to_string_lossy()));
    for argument in &manifest.launch.args {
        wrapper.push(' ');
        wrapper.push_str(&shell_quote(argument));
    }
    wrapper.push_str(" \"$@\"\n");
    Ok(wrapper)
}
fn archive_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("unsafe archive path")
    }
    Ok(path.to_path_buf())
}
fn set_archive_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))?;
    }
    Ok(())
}
fn device_identifier(model: &str) -> String {
    let mut identifier = String::new();
    let mut separator = false;
    for character in model.to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            identifier.push(character);
            separator = false;
        } else if !identifier.is_empty() && !separator {
            identifier.push('_');
            separator = true;
        }
    }
    identifier.trim_end_matches('_').to_string()
}

fn strip_device_id(hostname: &str) -> String {
    let Some(separator) = hostname.rfind(|character| character == '-' || character == '_') else {
        return hostname.to_string();
    };
    let (model, suffix) = hostname.split_at(separator);
    let device_id = &suffix[1..];
    if device_id.len() == 4
        && !model.is_empty()
        && device_id
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        model.to_string()
    } else {
        hostname.to_string()
    }
}
const MAX_UNPACKED_BYTES: u64 = 32 * 1024 * 1024;
const FREE_SPACE_RESERVE: u64 = 8 * 1024 * 1024;
fn unpack_budget(directory: &Path) -> Result<u64> {
    Ok(MAX_UNPACKED_BYTES.min(available_bytes(directory)?.saturating_sub(FREE_SPACE_RESERVE)))
}
#[cfg(unix)]
fn available_bytes(directory: &Path) -> Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(directory.as_os_str().as_bytes())?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("statvfs {}", directory.display()));
    }
    let stat = unsafe { stat.assume_init() };
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}
#[cfg(not(unix))]
fn available_bytes(_: &Path) -> Result<u64> {
    Ok(u64::MAX)
}
fn copy_limited(source: impl Read, output: &mut fs::File, remaining: &mut u64) -> Result<()> {
    let written = std::io::copy(&mut source.take(*remaining + 1), output)?;
    if written > *remaining {
        bail!("package does not fit: unpacked size is limited to 32 MiB and 8 MiB of free space is kept")
    }
    *remaining -= written;
    Ok(())
}
fn extract_zip_yap(bytes: &[u8], stage: &Path, mut remaining: u64) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("unsafe archive path"))?
            .to_owned();
        let destination = stage.join(path);
        if entry.is_dir() {
            fs::create_dir_all(destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mode = entry.unix_mode();
            let mut output = fs::File::create(&destination)?;
            copy_limited(&mut entry, &mut output, &mut remaining)?;
            if let Some(mode) = mode {
                set_archive_mode(&destination, mode)?;
            }
        }
    }
    Ok(())
}
fn extract_tar_gz_yap(bytes: &[u8], stage: &Path, mut remaining: u64) -> Result<()> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    for item in archive.entries()? {
        let mut entry = item?;
        let path = archive_path(&entry.path()?)?;
        let destination = stage.join(path);
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            fs::create_dir_all(destination)?;
        } else if entry_type.is_file() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mode = entry.header().mode()?;
            let mut output = fs::File::create(&destination)?;
            copy_limited(&mut entry, &mut output, &mut remaining)?;
            set_archive_mode(&destination, mode)?;
        } else {
            bail!("unsupported TAR archive entry")
        }
    }
    Ok(())
}
fn compatibility_error(m: &Manifest, d: &DeviceProfile) -> Option<String> {
    if !m.compatibility.models.is_empty() && !m.compatibility.models.iter().any(|x| x == &d.model) {
        return Some(format!(
            "requires model [{}], detected {}",
            m.compatibility.models.join(", "),
            d.model
        ));
    };
    let present: HashSet<_> = d.features.iter().collect();
    let missing: Vec<_> = m
        .compatibility
        .features
        .iter()
        .filter(|x| !present.contains(x))
        .cloned()
        .collect();
    if missing.is_empty() {
        None
    } else {
        Some(format!("missing feature(s): {}", missing.join(", ")))
    }
}
fn detect_device(root: &Path, forced: Option<String>) -> DeviceProfile {
    if let Ok(raw) = fs::read_to_string(root.join("device.toml")) {
        if let Ok(mut p) = toml::from_str::<DeviceProfile>(&raw) {
            populate_capabilities(&mut p);
            return p;
        }
    }
    let arch = std::env::consts::ARCH.to_string();
    let hostname = forced.unwrap_or_else(|| {
        Command::new("uname")
            .arg("-n")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| "unknown".into())
    });
    let model = strip_device_id(&hostname);
    let mut features = vec!["runtime.lua.v1".into(), "plugin.yap.v1".into()];
    if arch.starts_with("arm") {
        features.push("arch.armhf".into())
    };
    if arch == "aarch64" {
        features.push("arch.aarch64".into())
    }
    let mut profile = DeviceProfile {
        id: device_identifier(&model),
        model,
        architecture: arch,
        led_count: 0,
        features,
        capabilities: HashMap::new(),
        source: "runtime (device capabilities supplied by standard Device API)".into(),
    };
    populate_capabilities(&mut profile);
    profile
}
fn apply_device_info(device: &mut DeviceProfile, info: DeviceInfo) {
    if !info.id.is_empty() {
        device.id = info.id;
    }
    if !info.model.is_empty() {
        device.model = info.model;
    }
    if !info.architecture.is_empty() {
        device.architecture = info.architecture;
    }
    device.led_count = info.led_count;
    device.features = info.features;
    device.capabilities = info.capabilities;
    populate_capabilities(device);
    for feature in ["runtime.lua.v1", "plugin.yap.v1"] {
        if !device.features.iter().any(|present| present == feature) {
            device.features.push(feature.into());
        }
    }
    device.source = "standard device API".into();
}
fn populate_capabilities(device: &mut DeviceProfile) {
    for feature in &device.features {
        device.capabilities.entry(feature.clone()).or_insert(true);
    }
    for capability in [
        "led",
        "led.off",
        "led.ring",
        "led.ring.rgb",
        "led.ring.segments",
        "led.pattern",
        "volume",
        "volume.get",
        "volume.set",
        "audio",
        "audio.play",
        "bluetooth",
        "bluetooth.status",
        "bluetooth.power",
        "bluetooth.scan",
        "bluetooth.pair",
        "bluetooth.connect",
        "bluetooth.disconnect",
        "bluetooth.remove",
        "display",
        "display.backlights",
        "display.backlight",
        "sensors",
        "sensors.list",
        "sensors.read",
        "sensors.proximity",
        "sensors.control",
        "sensors.ambient_light.configure",
        "clock",
        "clock.available",
        "clock.off",
        "clock.time",
        "clock.segments",
        "input",
        "input.devices",
        "input.next_event",
        "ipc.alarm",
        "ipc.speech",
        "wifi",
        "wifi.status",
        "wifi.scan",
        "wifi.networks",
        "wifi.ap",
    ] {
        device
            .capabilities
            .entry(capability.into())
            .or_insert_with(|| {
                device.features.iter().any(|feature| {
                    feature == capability || feature.starts_with(&format!("{capability}."))
                })
            });
    }
}
fn capability_available(device: &DeviceProfile, capability: &str) -> bool {
    device
        .capabilities
        .get(capability)
        .copied()
        .unwrap_or_else(|| {
            device.features.iter().any(|feature| {
                feature == capability || feature.starts_with(&format!("{capability}."))
            })
        })
}
fn set_lua_availability(
    lua: &Lua,
    table: &mlua::Table,
    device: &DeviceProfile,
    capability: &str,
) -> mlua::Result<()> {
    let available = capability_available(device, capability);
    table.set(
        "is_available",
        lua.create_function(move |_, ()| Ok(available))?,
    )
}
/// Replaces `path` via a synced sibling file so a power cut never leaves it truncated.
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    let temporary = path.with_extension("tmp");
    let mut file =
        fs::File::create(&temporary).with_context(|| format!("write {}", temporary.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::rename(&temporary, path).with_context(|| format!("replace {}", path.display()))
}
fn network_url_allowed(policy: &NetworkPolicy, value: &str) -> Result<Url> {
    let url = Url::parse(value).context("invalid URL")?;
    match url.scheme() {
        "https" => {}
        "http" if policy.allow_insecure_http => {}
        "http" => bail!("HTTP requires network.allow_insecure_http = true"),
        _ => bail!("only HTTP(S) URLs are allowed"),
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("URLs with credentials are not allowed")
    }
    let host = url
        .host_str()
        .context("URL host is required")?
        .to_ascii_lowercase();
    if host == "localhost" {
        bail!("localhost is not allowed")
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let local = match ip {
            std::net::IpAddr::V4(ip) => {
                ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_broadcast()
                    || ip.is_unspecified()
                    || ip.is_multicast()
            }
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unicast_link_local()
                    || ip.is_unspecified()
                    || ip.is_multicast()
            }
        };
        if local {
            bail!("local-network addresses are not allowed")
        }
    }
    let allowed = policy.allow.iter().any(|entry| {
        let entry = entry.to_ascii_lowercase();
        entry == "*"
            || entry == host
            || entry
                .strip_prefix("*.")
                .is_some_and(|suffix| host.ends_with(&format!(".{suffix}")))
    });
    if !allowed {
        bail!("URL host is not allowed by the package manifest")
    }
    Ok(url)
}
fn http_request(
    policy: &NetworkPolicy,
    method: &str,
    url: &str,
    body: Option<String>,
) -> Result<(u16, String)> {
    let url = network_url_allowed(policy, url)?;
    let method = reqwest::Method::from_bytes(method.as_bytes()).context("invalid HTTP method")?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .request(method, url)
        .body(body.unwrap_or_default())
        .send()?;
    let status = response.status().as_u16();
    let mut response_body = String::new();
    response
        .take(1_048_576)
        .read_to_string(&mut response_body)?;
    Ok((status, response_body))
}
fn validate_input_policy(m: &Manifest) -> Result<()> {
    if m.input.devices.is_empty() {
        return Ok(());
    }
    if !m.permissions.iter().any(|p| p == "input") {
        bail!("[input].devices requires the input permission")
    }
    if m.input.devices.len() > 16 {
        bail!("[input].devices lists more than 16 devices")
    }
    for name in &m.input.devices {
        if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            bail!("[input].devices contains an invalid device name")
        }
    }
    Ok(())
}
/// The device names a package declared; packages without a list get no input devices.
fn input_scope(m: &Manifest) -> Result<&[String]> {
    if m.input.devices.is_empty() {
        bail!("alice.input.next_event requires a non-empty [input].devices manifest list")
    }
    Ok(&m.input.devices)
}
fn validate_input_scope(requested: &[String], allowed: &[String]) -> Result<()> {
    if requested.is_empty() {
        bail!("input request must name at least one device")
    }
    if requested.iter().any(|name| !allowed.contains(name)) {
        bail!("input request includes a device not declared by the package")
    }
    Ok(())
}
fn validate_id(s: &str) -> Result<()> {
    if s.is_empty()
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid plugin id")
    };
    Ok(())
}
fn safe_relative(s: &str) -> Result<()> {
    let p = Path::new(s);
    if p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("unsafe path")
    };
    Ok(())
}
fn device_api_json(
    led: &Led,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    tokio::runtime::Handle::current().block_on(led.api_json(method, path, body))
}
fn device_api_string(
    led: &Led,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<String> {
    let value = device_api_json(led, method, path, body)?;
    match value {
        serde_json::Value::String(value) => Ok(value),
        serde_json::Value::Null => Ok(String::new()),
        value => Ok(serde_json::to_string(&value)?),
    }
}
fn encode_path_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
fn valid_mac(mac: &str) -> bool {
    mac.len() == 17
        && mac.bytes().enumerate().all(|(i, b)| {
            if [2, 5, 8, 11, 14].contains(&i) {
                b == b':'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
async fn run_plugin(app: App, id: String, event: String, cancelled: Arc<AtomicBool>) -> Result<()> {
    let device = app.device.lock().await.clone();
    let led_count = device.led_count;
    let (manifest, root, allowed) = {
        let p = app.plugins.lock().await;
        if !p.enabled.contains(&id) {
            bail!("plugin is disabled")
        };
        let m = p.manifest(&id)?;
        if let Some(reason) = compatibility_error(&m, &device) {
            bail!("plugin is incompatible: {reason}")
        };
        (
            m,
            p.root.join(&id),
            app.settings.lock().await.allow_plugin_binaries,
        )
    };
    let led = app.led.clone();
    let wifi = app.wifi.clone();
    task::spawn_blocking(move || -> Result<()> {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::default(),
        )?;
        let globals = lua.globals();
        let api = lua.create_table()?;
        let permissions: HashSet<_> = manifest.permissions.iter().map(String::as_str).collect();
        if permissions.contains("wifi") {
            let wifi_api = lua.create_table()?;
            set_lua_availability(&lua, &wifi_api, &device, "wifi")?;
            let status_wifi = wifi.clone();
            wifi_api.set(
                "status",
                lua.create_function(move |lua, ()| {
                    let value = status_wifi.status().map_err(mlua::Error::external)?;
                    lua.to_value(&value)
                })?,
            )?;
            let scan_wifi = wifi.clone();
            wifi_api.set(
                "scan",
                lua.create_function(move |_, ()| scan_wifi.scan().map_err(mlua::Error::external))?,
            )?;
            let add_wifi = wifi.clone();
            wifi_api.set(
                "add",
                lua.create_function(move |_, (ssid, password): (String, Option<String>)| {
                    add_wifi
                        .add(&ssid, password.as_deref().unwrap_or_default())
                        .map_err(mlua::Error::external)
                })?,
            )?;
            let forget_wifi = wifi.clone();
            wifi_api.set(
                "forget",
                lua.create_function(move |_, ssid: String| {
                    forget_wifi.forget(&ssid).map_err(mlua::Error::external)
                })?,
            )?;
            let setup_wifi = wifi.clone();
            wifi_api.set(
                "setup",
                lua.create_function(move |_, on: bool| {
                    setup_wifi.request(on).map_err(mlua::Error::external)
                })?,
            )?;
            api.set("wifi", wifi_api)?;
        }
        let cancelled_sleep = cancelled.clone();
        api.set(
            "sleep",
            lua.create_function(move |_, ms: u64| {
                if ms > 60_000 {
                    return Err(mlua::Error::external("sleep is limited to 60 seconds"));
                };
                let mut elapsed = 0;
                while elapsed < ms {
                    if cancelled_sleep.load(Ordering::Relaxed) {
                        return Err(mlua::Error::external("plugin stopped"));
                    }
                    let slice = (ms - elapsed).min(20);
                    std::thread::sleep(Duration::from_millis(slice));
                    elapsed += slice;
                }
                Ok(())
            })?,
        )?;
        if permissions.contains("led") {
            let logo = lua.create_table()?;
            set_lua_availability(&lua, &logo, &device, "led.ring.rgb")?;
            let logo_led = led.clone();
            let logo_cancelled = cancelled.clone();
            logo.set(
                "set",
                lua.create_function(move |_, colors: mlua::Table| {
                    if logo_cancelled.load(Ordering::Relaxed) {
                        return Err(mlua::Error::external("plugin stopped"));
                    }
                    let colors = colors
                        .sequence_values::<String>()
                        .collect::<mlua::Result<Vec<_>>>()?;
                    tokio::runtime::Handle::current()
                        .block_on(logo_led.draw(colors))
                        .map_err(mlua::Error::external)
                })?,
            )?;
            let logo_led = led.clone();
            logo.set(
                "off",
                lua.create_function(move |_, ()| {
                    tokio::runtime::Handle::current()
                        .block_on(logo_led.off())
                        .map_err(mlua::Error::external)
                })?,
            )?;
            api.set("logo", logo)?;
            let led_api = lua.create_table()?;
            set_lua_availability(&lua, &led_api, &device, "led")?;
            let ring = lua.create_table()?;
            set_lua_availability(&lua, &ring, &device, "led.ring")?;
            let rgb = lua.create_table()?;
            set_lua_availability(&lua, &rgb, &device, "led.ring.rgb")?;
            ring.set("rgb", rgb)?;
            let segments = lua.create_table()?;
            set_lua_availability(&lua, &segments, &device, "led.ring.segments")?;
            ring.set("segments", segments)?;
            led_api.set("ring", ring)?;
            api.set("led", led_api)?;
            let led2 = led.clone();
            let cancelled_set = cancelled.clone();
            api.set(
                "set",
                lua.create_function(move |_, colors: mlua::Table| {
                    if cancelled_set.load(Ordering::Relaxed) {
                        return Err(mlua::Error::external("plugin stopped"));
                    }
                    let v = colors
                        .sequence_values::<String>()
                        .collect::<mlua::Result<Vec<_>>>()?;
                    tokio::runtime::Handle::current()
                        .block_on(led2.draw(v))
                        .map_err(mlua::Error::external)
                })?,
            )?;
            let led3 = led.clone();
            api.set(
                "off",
                lua.create_function(move |_, ()| {
                    tokio::runtime::Handle::current()
                        .block_on(led3.off())
                        .map_err(mlua::Error::external)
                })?,
            )?;
            let pattern_root = root.clone();
            let cancelled_play = cancelled.clone();
            let pattern_led = led.clone();
            api.set(
                "play",
                lua.create_function(move |_, name: String| {
                    safe_relative(&name).map_err(mlua::Error::external)?;
                    tokio::runtime::Handle::current()
                        .block_on(pattern_led.play_led(pattern_root.join(name), &cancelled_play))
                        .map_err(mlua::Error::external)
                })?,
            )?;
        }
        if permissions.contains("volume") {
            let volume = lua.create_table()?;
            set_lua_availability(&lua, &volume, &device, "volume")?;
            let volume_led = led.clone();
            volume.set(
                "set",
                lua.create_function(move |_, percent: u8| {
                    if percent > 100 {
                        return Err(mlua::Error::external("volume must be 0..100"));
                    };
                    device_api_string(
                        &volume_led,
                        Method::PUT,
                        "volume",
                        Some(serde_json::json!({"percent": percent})),
                    )
                    .map_err(mlua::Error::external)
                })?,
            )?;
            let volume_led = led.clone();
            volume.set(
                "get",
                lua.create_function(move |_, ()| {
                    device_api_string(&volume_led, Method::GET, "volume", None)
                        .map_err(mlua::Error::external)
                })?,
            )?;
            api.set("volume", volume)?;
        }
        if permissions.contains("audio") {
            let audio = lua.create_table()?;
            set_lua_availability(&lua, &audio, &device, "audio")?;
            let audio_root = root.clone();
            let audio_led = led.clone();
            let audio_cancelled = cancelled.clone();
            audio.set(
                "play",
                lua.create_function(move |_, file: String| {
                    safe_relative(&file).map_err(mlua::Error::external)?;
                    let path = audio_root.join(file);
                    if !path.is_file() {
                        return Err(mlua::Error::external("audio file not found"));
                    };
                    if fs::metadata(&path).map_err(mlua::Error::external)?.len() > 64 * 1024 * 1024
                    {
                        return Err(mlua::Error::external("audio file exceeds 64 MiB"));
                    }
                    let contents = fs::read(path).map_err(mlua::Error::external)?;
                    audio::validate(&contents).map_err(mlua::Error::external)?;
                    tokio::runtime::Handle::current()
                        .block_on(audio_led.api_bytes(
                            "audio/play",
                            contents,
                            audio_cancelled.clone(),
                        ))
                        .map_err(mlua::Error::external)
                })?,
            )?;
            api.set("audio", audio)?;
        }
        if permissions.contains("bluetooth") {
            let bt = lua.create_table()?;
            set_lua_availability(&lua, &bt, &device, "bluetooth")?;
            let bluetooth_led = led.clone();
            bt.set(
                "status",
                lua.create_function(move |_, ()| {
                    device_api_string(&bluetooth_led, Method::GET, "bluetooth/status", None)
                        .map_err(mlua::Error::external)
                })?,
            )?;
            let bluetooth_led = led.clone();
            bt.set(
                "power",
                lua.create_function(move |_, on: bool| {
                    device_api_string(
                        &bluetooth_led,
                        Method::PUT,
                        "bluetooth/power",
                        Some(serde_json::json!({"on": on})),
                    )
                    .map_err(mlua::Error::external)
                })?,
            )?;
            let bluetooth_led = led.clone();
            bt.set(
                "scan",
                lua.create_function(move |_, on: bool| {
                    device_api_string(
                        &bluetooth_led,
                        Method::PUT,
                        "bluetooth/scan",
                        Some(serde_json::json!({"on": on})),
                    )
                    .map_err(mlua::Error::external)
                })?,
            )?;
            for action in ["pair", "connect", "disconnect", "remove"] {
                let a = action.to_string();
                let bluetooth_led = led.clone();
                bt.set(
                    action,
                    lua.create_function(move |_, address: String| {
                        if !valid_mac(&address) {
                            return Err(mlua::Error::external("invalid Bluetooth MAC"));
                        };
                        device_api_string(
                            &bluetooth_led,
                            Method::POST,
                            &format!("bluetooth/{a}"),
                            Some(serde_json::json!({"address": address})),
                        )
                        .map_err(mlua::Error::external)
                    })?,
                )?;
            }
            api.set("bluetooth", bt)?;
        }
        if permissions.contains("display") {
            let display = lua.create_table()?;
            set_lua_availability(&lua, &display, &device, "display")?;
            let display_led = led.clone();
            display.set(
                "backlights",
                lua.create_function(move |lua, ()| {
                    let value =
                        device_api_json(&display_led, Method::GET, "display/backlights", None)
                            .map_err(mlua::Error::external)?;
                    lua.to_value(&value)
                })?,
            )?;
            let display_led = led.clone();
            display.set(
                "set_backlight",
                lua.create_function(move |_, (name, value): (String, u32)| {
                    validate_id(&name).map_err(mlua::Error::external)?;
                    device_api_json(
                        &display_led,
                        Method::PUT,
                        "display/backlight",
                        Some(serde_json::json!({"name": name, "value": value})),
                    )
                    .map(|_| ())
                    .map_err(mlua::Error::external)
                })?,
            )?;
            api.set("display", display)?;
        }
        if permissions.contains("sensors") {
            let sensors = lua.create_table()?;
            set_lua_availability(&lua, &sensors, &device, "sensors")?;
            let sensors_led = led.clone();
            sensors.set(
                "list",
                lua.create_function(move |lua, ()| {
                    let value = device_api_json(&sensors_led, Method::GET, "sensors", None)
                        .map_err(mlua::Error::external)?;
                    lua.to_value(&value)
                })?,
            )?;
            let sensors_led = led.clone();
            sensors.set(
                "read",
                lua.create_function(move |lua, id: String| {
                    let path = format!("sensors/{}", encode_path_segment(&id));
                    let value = device_api_json(&sensors_led, Method::GET, &path, None)
                        .map_err(mlua::Error::external)?;
                    lua.to_value(&value)
                })?,
            )?;
            if permissions.contains("sensors.control") {
                let control = lua.create_table()?;
                set_lua_availability(&lua, &control, &device, "sensors.control")?;
                sensors.set("control", control)?;
                let sensors_led = led.clone();
                sensors.set(
                    "configure_ambient_light",
                    lua.create_function(
                        move |_, (name, gain, integration_ms): (String, u8, u16)| {
                            if ![1, 3, 6, 9, 18].contains(&gain) {
                                return Err(mlua::Error::external("gain must be 1, 3, 6, 9 or 18"));
                            }
                            if ![25, 50, 100, 200, 400].contains(&integration_ms) {
                                return Err(mlua::Error::external(
                                    "integration time must be 25, 50, 100, 200 or 400 ms",
                                ));
                            }
                            device_api_json(
                                &sensors_led,
                                Method::POST,
                                "sensors/ambient-light/configure",
                                Some(serde_json::json!({
                                    "name": name,
                                    "gain": gain,
                                    "integration_ms": integration_ms
                                })),
                            )
                            .map(|_| ())
                            .map_err(mlua::Error::external)
                        },
                    )?,
                )?;
            }
            api.set("sensors", sensors)?;
        }
        if permissions.contains("clock") {
            let clock = lua.create_table()?;
            set_lua_availability(&lua, &clock, &device, "clock")?;
            let segments = lua.create_table()?;
            set_lua_availability(&lua, &segments, &device, "clock.segments")?;
            let segment_call = lua.create_table()?;
            let segment_led = led.clone();
            segment_call.set(
                "__call",
                lua.create_function(move |_, (_segments, levels): (mlua::Table, mlua::Table)| {
                    let mut frame = HashMap::new();
                    for pair in levels.pairs::<String, u8>() {
                        let (segment, value) = pair?;
                        frame.insert(segment, value);
                    }
                    device_api_json(
                        &segment_led,
                        Method::PUT,
                        "clock",
                        Some(serde_json::json!({"segments": frame})),
                    )
                    .map(|_| ())
                    .map_err(mlua::Error::external)
                })?,
            )?;
            segments.set_metatable(Some(segment_call));
            clock.set("segments", segments)?;
            let clock_available = capability_available(&device, "clock");
            clock.set(
                "available",
                lua.create_function(move |_, ()| Ok(clock_available))?,
            )?;
            let time_led = led.clone();
            clock.set(
                "time",
                lua.create_function(move |_, (time, brightness): (String, Option<u8>)| {
                    device_api_json(
                        &time_led,
                        Method::PUT,
                        "clock",
                        Some(serde_json::json!({
                            "time": time,
                            "brightness": brightness.unwrap_or(170)
                        })),
                    )
                    .map(|_| ())
                    .map_err(mlua::Error::external)
                })?,
            )?;
            let off_led = led.clone();
            clock.set(
                "off",
                lua.create_function(move |_, ()| {
                    device_api_json(
                        &off_led,
                        Method::PUT,
                        "clock",
                        Some(serde_json::json!({"off": true})),
                    )
                    .map(|_| ())
                    .map_err(mlua::Error::external)
                })?,
            )?;
            api.set("clock", clock)?;
        }
        if permissions.contains("input") {
            let input = lua.create_table()?;
            set_lua_availability(&lua, &input, &device, "input")?;
            let input_devices_manifest = manifest.clone();
            let input_led = led.clone();
            input.set(
                "devices",
                lua.create_function(move |lua, ()| {
                    let allowed: HashSet<_> = input_scope(&input_devices_manifest)
                        .map_err(mlua::Error::external)?
                        .iter()
                        .map(String::as_str)
                        .collect();
                    let value = device_api_json(&input_led, Method::GET, "input/devices", None)
                        .map_err(mlua::Error::external)?;
                    let names: Vec<String> =
                        serde_json::from_value(value).map_err(mlua::Error::external)?;
                    lua.to_value(
                        &names
                            .into_iter()
                            .filter(|name| allowed.contains(name.as_str()))
                            .collect::<Vec<_>>(),
                    )
                })?,
            )?;
            let input_manifest = manifest.clone();
            let cancelled_input = cancelled.clone();
            let input_led = led.clone();
            input.set(
                "next_event",
                lua.create_function(move |lua, timeout_ms: u64| {
                    if timeout_ms > 60_000 {
                        return Err(mlua::Error::external(
                            "input timeout is limited to 60 seconds",
                        ));
                    }
                    if cancelled_input.load(Ordering::Relaxed) {
                        return Err(mlua::Error::external("plugin stopped"));
                    }
                    let devices = input_scope(&input_manifest).map_err(mlua::Error::external)?;
                    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
                    loop {
                        if cancelled_input.load(Ordering::Relaxed) {
                            return Err(mlua::Error::external("plugin stopped"));
                        }
                        let wait_ms = if timeout_ms == 0 {
                            0
                        } else {
                            deadline
                                .saturating_duration_since(std::time::Instant::now())
                                .as_millis()
                                .min(100) as u64
                        };
                        let value = device_api_json(
                            &input_led,
                            Method::POST,
                            "input/next-event",
                            Some(serde_json::json!({
                                "devices": devices,
                                "timeout_ms": wait_ms
                            })),
                        )
                        .map_err(mlua::Error::external)?;
                        if !value.is_null() {
                            return lua.to_value(&value);
                        }
                        if timeout_ms == 0 || std::time::Instant::now() >= deadline {
                            return Ok(mlua::Value::Nil);
                        }
                    }
                })?,
            )?;
            api.set("input", input)?;
        }
        if permissions.contains("network") {
            if manifest.network.allow.is_empty() {
                bail!("network permission requires a non-empty [network].allow manifest list")
            }
            let http = lua.create_table()?;
            let get_policy = manifest.network.clone();
            http.set(
                "get",
                lua.create_function(move |lua, url: String| {
                    let (status, body) = http_request(&get_policy, "GET", &url, None)
                        .map_err(mlua::Error::external)?;
                    let result = lua.create_table()?;
                    result.set("status", status)?;
                    result.set("body", body)?;
                    Ok(result)
                })?,
            )?;
            let post_policy = manifest.network.clone();
            http.set(
                "post",
                lua.create_function(move |lua, (url, body): (String, String)| {
                    if body.len() > 1_048_576 {
                        return Err(mlua::Error::external("request body exceeds 1 MiB"));
                    }
                    let (status, body) = http_request(&post_policy, "POST", &url, Some(body))
                        .map_err(mlua::Error::external)?;
                    let result = lua.create_table()?;
                    result.set("status", status)?;
                    result.set("body", body)?;
                    Ok(result)
                })?,
            )?;
            api.set("http", http)?;
        }
        if permissions.contains("ipc.alarm")
            || permissions.contains("ipc.speech")
            || permissions.contains("ipc.read")
            || permissions.contains("ipc.control")
        {
            let ipc = lua.create_table()?;
            if permissions.contains("ipc.alarm") {
                let alarm = lua.create_table()?;
                set_lua_availability(&lua, &alarm, &device, "ipc.alarm")?;
                ipc.set("alarm", alarm)?;
                let ipc_led = led.clone();
                ipc.set(
                    "next_alarm",
                    lua.create_function(move |lua, timeout_ms: u64| {
                        if timeout_ms > 60_000 {
                            return Err(mlua::Error::external(
                                "IPC timeout is limited to 60 seconds",
                            ));
                        }
                        let value = device_api_json(
                            &ipc_led,
                            Method::POST,
                            "ipc/alarm/next",
                            Some(serde_json::json!({"timeout_ms": timeout_ms})),
                        )
                        .map_err(mlua::Error::external)?;
                        if value.is_null() {
                            Ok(mlua::Value::Nil)
                        } else {
                            lua.to_value(&value)
                        }
                    })?,
                )?;
            }
            if permissions.contains("ipc.speech") {
                let speech = lua.create_table()?;
                set_lua_availability(&lua, &speech, &device, "ipc.speech")?;
                ipc.set("speech", speech)?;
                let ipc_led = led.clone();
                ipc.set(
                    "next_speech",
                    lua.create_function(move |lua, timeout_ms: u64| {
                        if timeout_ms > 60_000 {
                            return Err(mlua::Error::external(
                                "IPC timeout is limited to 60 seconds",
                            ));
                        }
                        let value = device_api_json(
                            &ipc_led,
                            Method::POST,
                            "ipc/speech/next",
                            Some(serde_json::json!({"timeout_ms": timeout_ms})),
                        )
                        .map_err(mlua::Error::external)?;
                        if value.is_null() {
                            Ok(mlua::Value::Nil)
                        } else {
                            lua.to_value(&value)
                        }
                    })?,
                )?;
            }
            if permissions.contains("ipc.read") || permissions.contains("ipc.control") {
                if manifest.ipc.services.is_empty() {
                    bail!("ipc.read and ipc.control require a non-empty [ipc].services list")
                }
            }
            if permissions.contains("ipc.read") {
                let read_services = manifest.ipc.services.clone();
                let ipc_cancelled = cancelled.clone();
                ipc.set(
                    "next",
                    lua.create_function(move |lua, (service, timeout_ms): (String, u64)| {
                        ipc::service_allowed(&read_services, &service)
                            .and_then(|_| ipc::receive(&service, timeout_ms, &ipc_cancelled))
                            .map_err(mlua::Error::external)?
                            .map(|payload| lua.create_string(&payload))
                            .transpose()?
                            .map(mlua::Value::String)
                            .map_or(Ok(mlua::Value::Nil), Ok)
                    })?,
                )?;
            }
            if permissions.contains("ipc.control") {
                let control_services = manifest.ipc.services.clone();
                let ipc_cancelled = cancelled.clone();
                ipc.set(
                    "send",
                    lua.create_function(move |_, (service, payload): (String, mlua::String)| {
                        ipc::service_allowed(&control_services, &service)
                            .and_then(|_| {
                                ipc::send(&service, payload.as_bytes().as_ref(), &ipc_cancelled)
                            })
                            .map_err(mlua::Error::external)
                    })?,
                )?;
            }
            if permissions.contains("ipc.read") && permissions.contains("ipc.control") {
                let request_services = manifest.ipc.services.clone();
                let ipc_cancelled = cancelled.clone();
                ipc.set(
                    "request",
                    lua.create_function(
                        move |lua, (service, payload, timeout_ms): (String, mlua::String, u64)| {
                            ipc::service_allowed(&request_services, &service)
                                .and_then(|_| {
                                    ipc::request(
                                        &service,
                                        payload.as_bytes().as_ref(),
                                        timeout_ms,
                                        &ipc_cancelled,
                                    )
                                })
                                .map_err(mlua::Error::external)?
                                .map(|response| lua.create_string(&response))
                                .transpose()?
                                .map(mlua::Value::String)
                                .map_or(Ok(mlua::Value::Nil), Ok)
                        },
                    )?,
                )?;
            }
            api.set("ipc", ipc)?;
        }
        if permissions.contains("system") {
            let system = lua.create_table()?;
            system.set(
                "hostname",
                lua.create_function(|_, ()| {
                    fs::read_to_string("/etc/hostname")
                        .map(|x| x.trim().to_string())
                        .map_err(mlua::Error::external)
                })?,
            )?;
            system.set(
                "uptime",
                lua.create_function(|_, ()| {
                    fs::read_to_string("/proc/uptime").map_err(mlua::Error::external)
                })?,
            )?;
            system.set(
                "meminfo",
                lua.create_function(|_, ()| {
                    fs::read_to_string("/proc/meminfo").map_err(mlua::Error::external)
                })?,
            )?;
            api.set("system", system)?;
        }
        if permissions.contains("exec") {
            let plugin_root = root.clone();
            api.set(
                "exec",
                lua.create_function(move |_, (program, args): (String, Vec<String>)| {
                    if !allowed {
                        return Err(mlua::Error::external(
                            "binary execution disabled by administrator",
                        ));
                    };
                    safe_relative(&program).map_err(mlua::Error::external)?;
                    let output = Command::new(plugin_root.join(program))
                        .args(args)
                        .current_dir(&plugin_root)
                        .output()
                        .map_err(mlua::Error::external)?;
                    Ok(String::from_utf8_lossy(&output.stdout).to_string())
                })?,
            )?;
        }
        globals.set("alice", api)?;
        globals.set("event", event)?;
        let lua_device = lua.create_table()?;
        lua_device.set("model", device.model)?;
        lua_device.set("id", device.id)?;
        lua_device.set("architecture", device.architecture)?;
        lua_device.set("led_count", led_count)?;
        let lua_features = lua.create_table()?;
        for (i, feature) in device.features.iter().enumerate() {
            lua_features.set(i + 1, feature.as_str())?;
        }
        lua_device.set("features", lua_features)?;
        let lua_capabilities = lua.create_table()?;
        for (capability, available) in &device.capabilities {
            lua_capabilities.set(capability.as_str(), *available)?;
        }
        lua_device.set("capabilities", lua_capabilities)?;
        globals.set("device", lua_device)?;
        let launch_path = manifest.launch.path(&manifest).to_string();
        let source = fs::read_to_string(root.join(&launch_path))?;
        lua.load(&source).set_name(&launch_path).exec()?;
        Ok(())
    })
    .await??;
    Ok(())
}

async fn run_binary(
    app: App,
    id: String,
    invocation_args: Vec<String>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let device = app.device.lock().await.clone();
    let (manifest, root) = {
        let plugins = app.plugins.lock().await;
        if !plugins.enabled.contains(&id) {
            bail!("package is disabled")
        }
        let manifest = plugins.manifest(&id)?;
        if let Some(reason) = compatibility_error(&manifest, &device) {
            bail!("package is incompatible: {reason}")
        }
        if !manifest
            .permissions
            .iter()
            .any(|permission| permission == "exec")
        {
            bail!("binary packages require the exec permission")
        }
        (manifest, plugins.root.join(&id))
    };
    let token = Uuid::new_v4().to_string();
    app.runtime_tokens
        .lock()
        .await
        .insert(id.clone(), token.clone());
    let api_url = format!("{}/api/runtime/{id}", app.runtime_api_url);
    let task_id = id.clone();
    let result = task::spawn_blocking(move || -> Result<()> {
        let executable = manifest.launch.path(&manifest).to_string();
        safe_relative(&executable)?;
        let log = Arc::new(std::sync::Mutex::new(RotatingLog::open(
            root.join("runtime.log"),
        )?));
        let (interpreter, program) = launch_program(&manifest, &root, &executable);
        let mut command = match interpreter {
            Some(interpreter) => {
                let mut command = Command::new(interpreter);
                command.arg(program);
                command
            }
            None => Command::new(program),
        };
        let mut child = command
            .args(&manifest.launch.args)
            .args(invocation_args)
            .current_dir(&root)
            .env("LD_LIBRARY_PATH", binary_library_path(&manifest, &root))
            .env("YAM_API_URL", api_url)
            .env("YAM_API_TOKEN", token)
            .env("YAM_PLUGIN_ID", task_id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pumps = [
            pump_log(child.stdout.take(), log.clone()),
            pump_log(child.stderr.take(), log),
        ];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
            if let Some(status) = child.try_wait()? {
                for _ in 0..20 {
                    if pumps.iter().all(std::thread::JoinHandle::is_finished) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                if status.success() {
                    return Ok(());
                }
                bail!("binary exited with {status}")
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })
    .await?;
    app.runtime_tokens.lock().await.remove(&id);
    result
}

// Packages converted before `launch.interpreter` / `launch.library_paths`
// existed carry an apkg `<id>/<id>-run` script and `<prefix>/lib` next to
// `<prefix>/bin`; both are only guessed when the manifest is silent.
fn launch_program(
    manifest: &Manifest,
    root: &Path,
    executable: &str,
) -> (Option<&'static str>, PathBuf) {
    if manifest.launch.interpreter.as_deref() == Some("sh") {
        return (Some("sh"), root.join(executable));
    }
    let legacy_runner = root.join(&manifest.id).join(format!("{}-run", manifest.id));
    if manifest.launch.interpreter.is_none() && legacy_runner.is_file() {
        return (Some("sh"), legacy_runner);
    }
    (None, root.join(executable))
}
struct RotatingLog {
    path: PathBuf,
    file: fs::File,
    size: u64,
}
impl RotatingLog {
    const LIMIT: u64 = 1_048_576;
    fn open(path: PathBuf) -> std::io::Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let size = file.metadata()?.len();
        Ok(Self { path, file, size })
    }
    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        if self.size > Self::LIMIT {
            fs::rename(&self.path, self.path.with_extension("log.1"))?;
            *self = Self::open(self.path.clone())?;
        }
        self.file.write_all(data)?;
        self.size += data.len() as u64;
        Ok(())
    }
}
fn pump_log(
    source: Option<impl Read + Send + 'static>,
    log: Arc<std::sync::Mutex<RotatingLog>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let Some(mut source) = source else {
            return;
        };
        let mut buffer = [0; 8192];
        while let Ok(read @ 1..) = source.read(&mut buffer) {
            let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
            if log.write(&buffer[..read]).is_err() {
                return;
            }
        }
    })
}
fn binary_library_path(manifest: &Manifest, root: &Path) -> String {
    let bundled = binary_library_directories(manifest, root);
    let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
    bundled
        .iter()
        .map(|directory| directory.display().to_string())
        .chain((!existing.is_empty()).then_some(existing))
        .collect::<Vec<_>>()
        .join(":")
}
fn binary_library_directories(manifest: &Manifest, root: &Path) -> Vec<PathBuf> {
    if manifest.launch.library_paths.is_empty() {
        Path::new(manifest.launch.path(manifest))
            .parent()
            .and_then(Path::parent)
            .map(|directory| root.join(directory).join("lib"))
            .filter(|directory| directory.is_dir())
            .into_iter()
            .collect()
    } else {
        manifest
            .launch
            .library_paths
            .iter()
            .map(|directory| root.join(directory))
            .collect()
    }
}

async fn run_package(
    app: App,
    id: String,
    invocation_args: Vec<String>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let kind = app.plugins.lock().await.manifest(&id)?.launch.kind;
    match kind {
        LaunchKind::Lua => run_plugin(app, id, "manual".into(), cancelled).await,
        LaunchKind::Binary => run_binary(app, id, invocation_args, cancelled).await,
    }
}

async fn start_package(app: App, id: String, invocation_args: Vec<String>) -> Result<()> {
    let cancelled = {
        let mut plugins = app.plugins.lock().await;
        if !plugins.enabled.contains(&id) {
            bail!("package is disabled")
        }
        let manifest = plugins.manifest(&id)?;
        if manifest.launch.kind == LaunchKind::Binary
            && !app.settings.lock().await.allow_plugin_binaries
        {
            bail!("binary execution disabled by administrator")
        }
        if !invocation_args.is_empty() {
            if manifest.launch.kind != LaunchKind::Binary {
                bail!("only binary packages accept invocation arguments")
            }
            if manifest.launch.mode != LaunchMode::Oneshot {
                bail!("invocation arguments are only supported for oneshot packages")
            }
        }
        if let Some(previous) = plugins.running.remove(&id) {
            previous.store(true, Ordering::Relaxed)
        }
        let token = Arc::new(AtomicBool::new(false));
        plugins.running.insert(id.clone(), token.clone());
        token
    };
    let task_app = app.clone();
    let task_id = id.clone();
    tokio::spawn(async move {
        if let Err(error) = run_package(
            task_app.clone(),
            task_id.clone(),
            invocation_args,
            cancelled.clone(),
        )
        .await
        {
            if !cancelled.load(Ordering::Relaxed) {
                tracing::error!(package = %task_id, %error, "package execution failed")
            }
        }
        let mut plugins = task_app.plugins.lock().await;
        if plugins
            .running
            .get(&task_id)
            .is_some_and(|current| Arc::ptr_eq(current, &cancelled))
        {
            plugins.running.remove(&task_id);
        }
    });
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(UI)
}
async fn list(State(a): State<App>) -> Result<Json<Vec<PluginInfo>>, ApiError> {
    let d = a.device.lock().await.clone();
    Ok(Json(a.plugins.lock().await.list(&d)?))
}
async fn install(State(a): State<App>, mut form: Multipart) -> Result<Json<Manifest>, ApiError> {
    while let Some(f) = form.next_field().await? {
        if f.name() == Some("package") {
            let b = f.bytes().await?;
            let m = a.plugins.lock().await.install(&b)?;
            return Ok(Json(m));
        }
    }
    Err(ApiError::new(anyhow!("missing package field")))
}

#[derive(Deserialize)]
struct RegistryInstall {
    id: String,
    version: Option<String>,
    registry: String,
}

async fn install_registry(
    State(a): State<App>,
    Json(request): Json<RegistryInstall>,
) -> Result<Json<Manifest>, ApiError> {
    let package = task::spawn_blocking(move || {
        download_registry_package(&request.registry, &request.id, request.version.as_deref())
    })
    .await
    .context("registry download task failed")??;
    let manifest = a.plugins.lock().await.install(&package)?;
    Ok(Json(manifest))
}

fn download_registry_package(registry: &str, id: &str, version: Option<&str>) -> Result<Vec<u8>> {
    let index_url = if registry.trim_end_matches('/').ends_with("index.json") {
        registry.to_string()
    } else {
        format!("{}/index.json", registry.trim_end_matches('/'))
    };
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()?;
    let index: serde_json::Value = client.get(&index_url).send()?.error_for_status()?.json()?;
    let packages = index["packages"]
        .as_array()
        .context("invalid registry package index")?;
    let entry = match version {
        Some(version) => packages
            .iter()
            .find(|entry| entry["id"] == id && entry["version"] == version),
        None => packages
            .iter()
            .filter(|entry| entry["id"] == id)
            .max_by(|left, right| {
                registry_version(
                    left["version"].as_str().unwrap_or(""),
                    right["version"].as_str().unwrap_or(""),
                )
            }),
    }
    .context("package version not found in registry")?;
    let package_url = entry["url"]
        .as_str()
        .context("registry entry has no package URL")?;
    let package_url = Url::parse(package_url)
        .or_else(|_| Url::parse(&index_url)?.join(package_url))?
        .to_string();
    let package = client
        .get(package_url)
        .send()?
        .error_for_status()?
        .bytes()?
        .to_vec();
    let expected = entry["sha256"]
        .as_str()
        .context("registry entry has no SHA-256")?;
    if format!("{:x}", Sha256::digest(&package)) != expected {
        bail!("package checksum mismatch")
    }
    Ok(package)
}

fn registry_version(left: &str, right: &str) -> std::cmp::Ordering {
    let numbers = |value: &str| {
        value
            .trim_start_matches('v')
            .split(|character: char| character == '.' || character == '-')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let left_numbers = numbers(left);
    let right_numbers = numbers(right);
    for index in 0..left_numbers.len().max(right_numbers.len()) {
        match left_numbers
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right_numbers.get(index).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    left.cmp(right)
}
async fn toggle(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    let mut p = a.plugins.lock().await;
    let manifest = p.manifest(&id)?;
    let enabled = p.enabled.insert(id.clone());
    if !enabled {
        p.enabled.remove(&id);
        if let Some(token) = p.running.remove(&id) {
            token.store(true, Ordering::Relaxed);
        }
    }
    p.save()?;
    drop(p);
    if enabled && manifest.launch.mode == LaunchMode::Service {
        start_package(a, id, Vec::new()).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_logo(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(frame): Json<Frame>,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "led").await?;
    a.led.draw(frame.colors).await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_logo_off(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "led").await?;
    a.led.off().await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    a.plugins.lock().await.remove(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn invoke(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    invocation: Option<Json<Invocation>>,
) -> Result<StatusCode, ApiError> {
    start_package(
        a,
        id,
        invocation.map(|value| value.0.args).unwrap_or_default(),
    )
    .await?;
    Ok(StatusCode::ACCEPTED)
}
async fn stop_plugin(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    let mut plugins = a.plugins.lock().await;
    let Some(token) = plugins.running.remove(&id) else {
        return Ok(StatusCode::NO_CONTENT);
    };
    token.store(true, Ordering::Relaxed);
    let uses_led = plugins
        .manifest(&id)?
        .permissions
        .iter()
        .any(|p| p == "led");
    drop(plugins);
    if uses_led {
        a.led.off().await.map_err(ApiError::backend)?;
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn package_logs(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
) -> Result<String, ApiError> {
    validate_id(&id)?;
    let path = a.plugins.lock().await.root.join(id).join("runtime.log");
    Ok(fs::read_to_string(path).unwrap_or_default())
}
async fn runtime_permission(
    app: &App,
    id: &str,
    headers: &HeaderMap,
    permission: &str,
) -> Result<(), ApiError> {
    validate_id(id)?;
    let token = headers
        .get("x-yam-token")
        .and_then(|value| value.to_str().ok())
        .context("missing X-YAM-Token")?;
    if app.runtime_tokens.lock().await.get(id).map(String::as_str) != Some(token) {
        return Err(ApiError::new(anyhow!("invalid runtime token")));
    }
    let manifest = app.plugins.lock().await.manifest(id)?;
    if !manifest.permissions.iter().any(|item| item == permission) {
        return Err(ApiError::new(anyhow!(
            "package lacks {permission} permission"
        )));
    }
    Ok(())
}
async fn runtime_device(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<DeviceProfile>, ApiError> {
    runtime_permission(&a, &id, &headers, "exec").await?;
    Ok(Json(a.device.lock().await.clone()))
}
async fn runtime_sensors(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<SensorInfo>>, ApiError> {
    runtime_permission(&a, &id, &headers, "sensors").await?;
    get_sensors(State(a)).await
}
async fn runtime_sensor_read(
    State(a): State<App>,
    AxumPath((id, sensor)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<SensorInfo>, ApiError> {
    runtime_permission(&a, &id, &headers, "sensors").await?;
    let value = a
        .led
        .api_json(Method::GET, &format!("sensors/{sensor}"), None)
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(serde_json::from_value(value)?))
}
async fn runtime_audio_play(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "audio").await?;
    audio::validate(&body).map_err(ApiError::new)?;
    let cancelled = a
        .plugins
        .lock()
        .await
        .running
        .get(&id)
        .cloned()
        .context("plugin is not running")
        .map_err(ApiError::new)?;
    a.led
        .api_bytes("audio/play", body.to_vec(), cancelled)
        .await
        .map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_led(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(frame): Json<Frame>,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "led").await?;
    a.led.draw(frame.colors).await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_led_off(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "led").await?;
    a.led.off().await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_clock(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ClockRequest>,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "clock").await?;
    a.led
        .api_json(Method::PUT, "clock", Some(serde_json::to_value(request)?))
        .await
        .map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_volume_get(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<u8>, ApiError> {
    runtime_permission(&a, &id, &headers, "volume").await?;
    let value = a
        .led
        .api_json(Method::GET, "volume", None)
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(serde_json::from_value(value)?))
}
async fn runtime_volume_set(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<VolumeRequest>,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "volume").await?;
    a.led
        .api_json(
            Method::PUT,
            "volume",
            Some(serde_json::json!({"percent": request.percent})),
        )
        .await
        .map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_bluetooth_status(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    runtime_permission(&a, &id, &headers, "bluetooth").await?;
    let value = a
        .led
        .api_json(Method::GET, "bluetooth/status", None)
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(value))
}
async fn runtime_bluetooth_power(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<BluetoothPowerRequest>,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "bluetooth").await?;
    a.led
        .api_json(
            Method::PUT,
            "bluetooth/power",
            Some(serde_json::json!({"on": request.on})),
        )
        .await
        .map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_input_devices(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, ApiError> {
    runtime_permission(&a, &id, &headers, "input").await?;
    let manifest = a.plugins.lock().await.manifest(&id)?;
    let allowed = input_scope(&manifest)?;
    let value = a
        .led
        .api_json(Method::GET, "input/devices", None)
        .await
        .map_err(ApiError::backend)?;
    let names: Vec<String> = serde_json::from_value(value)?;
    Ok(Json(
        names
            .into_iter()
            .filter(|name| allowed.contains(name))
            .collect(),
    ))
}
async fn runtime_input_next_event(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<RuntimeInputRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    runtime_permission(&a, &id, &headers, "input").await?;
    let manifest = a.plugins.lock().await.manifest(&id)?;
    let allowed = input_scope(&manifest)?;
    validate_input_scope(&request.devices, allowed).map_err(ApiError::new)?;
    let value = a
        .led
        .api_json(
            Method::POST,
            "input/next-event",
            Some(serde_json::json!({
                "devices": request.devices,
                "timeout_ms": request.timeout_ms
            })),
        )
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(value))
}
async fn runtime_wifi(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    runtime_permission(&a, &id, &headers, "wifi").await?;
    let wifi = a.wifi.clone();
    Ok(Json(wifi_blocking(move || wifi.status()).await?))
}
async fn runtime_wifi_scan(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    runtime_permission(&a, &id, &headers, "wifi").await?;
    let wifi = a.wifi.clone();
    let started = wifi_blocking(move || wifi.scan()).await?;
    Ok(Json(serde_json::json!({"started": started})))
}
async fn runtime_wifi_networks(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    runtime_permission(&a, &id, &headers, "wifi").await?;
    let wifi = a.wifi.clone();
    let status = wifi_blocking(move || wifi.status()).await?;
    Ok(Json(serde_json::json!({
        "saved": status["saved"],
        "around": status["around"]
    })))
}
async fn runtime_wifi_network(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<wifi_setup::NetworkRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    runtime_permission(&a, &id, &headers, "wifi").await?;
    wifi::check(&request.ssid, &request.password).map_err(ApiError::new)?;
    let wifi = a.wifi.clone();
    let leaving = wifi_blocking(move || wifi.add(&request.ssid, &request.password)).await?;
    Ok(Json(serde_json::json!({"leaving": leaving})))
}
async fn runtime_wifi_forget(
    State(a): State<App>,
    AxumPath((id, ssid)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "wifi").await?;
    let wifi = a.wifi.clone();
    wifi_blocking(move || wifi.forget(&ssid)).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn runtime_wifi_setup(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<WifiSetupRequest>,
) -> Result<StatusCode, ApiError> {
    runtime_permission(&a, &id, &headers, "wifi").await?;
    wifi_setup_request(a.wifi.clone(), request.on).await?;
    Ok(StatusCode::ACCEPTED)
}
async fn runtime_wifi_setup_status(
    State(a): State<App>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    runtime_permission(&a, &id, &headers, "wifi").await?;
    let wifi = a.wifi.clone();
    let status = wifi_blocking(move || wifi.status()).await?;
    Ok(Json(status["setup"].clone()))
}
async fn get_settings(State(a): State<App>) -> Json<Settings> {
    Json(a.settings.lock().await.clone())
}
async fn get_device(State(a): State<App>) -> Json<DeviceProfile> {
    Json(a.device.lock().await.clone())
}
async fn get_sensors(State(a): State<App>) -> Result<Json<Vec<SensorInfo>>, ApiError> {
    let value = a
        .led
        .api_json(Method::GET, "sensors", None)
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(serde_json::from_value(value)?))
}
async fn get_sensor(
    State(a): State<App>,
    AxumPath(sensor): AxumPath<String>,
) -> Result<Json<SensorInfo>, ApiError> {
    let value = a
        .led
        .api_json(Method::GET, &format!("sensors/{sensor}"), None)
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(serde_json::from_value(value)?))
}
async fn get_clock(State(a): State<App>) -> Result<Json<ClockInfo>, ApiError> {
    let device = a.device.lock().await;
    Ok(Json(ClockInfo {
        available: capability_available(&device, "clock"),
        segments: Vec::new(),
    }))
}
async fn put_clock(
    State(a): State<App>,
    Json(request): Json<ClockRequest>,
) -> Result<StatusCode, ApiError> {
    if !request.off && request.time.is_none() && request.segments.is_none() {
        return Err(ApiError::new(anyhow!("pass time, segments or off=true")));
    }
    a.led
        .api_json(Method::PUT, "clock", Some(serde_json::to_value(request)?))
        .await
        .map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn put_device(
    State(a): State<App>,
    Json(mut d): Json<DeviceProfile>,
) -> Result<StatusCode, ApiError> {
    if d.model.trim().is_empty() || d.model.chars().any(char::is_control) {
        return Err(ApiError::new(anyhow!(
            "device model cannot be empty or contain control characters"
        )));
    }
    if d.id.is_empty() {
        d.id = device_identifier(&d.model);
    }
    validate_id(&d.id)?;
    write_atomic(
        &a.root.join("device.toml"),
        toml::to_string_pretty(&d)?.as_bytes(),
    )?;
    *a.device.lock().await = d;
    Ok(StatusCode::NO_CONTENT)
}
async fn put_settings(
    State(a): State<App>,
    Json(s): Json<Settings>,
) -> Result<StatusCode, ApiError> {
    if s.web.ui && !s.web.api {
        return Err(ApiError::new(anyhow!("web.ui requires web.api")));
    }
    s.wifi_setup.validate().map_err(ApiError::new)?;
    write_atomic(
        &a.root.join("settings.toml"),
        toml::to_string_pretty(&s)?.as_bytes(),
    )?;
    *a.settings.lock().await = s.clone();
    Ok(StatusCode::NO_CONTENT)
}
async fn led_set(State(a): State<App>, Json(f): Json<Frame>) -> Result<StatusCode, ApiError> {
    a.led.draw(f.colors).await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn led_off(State(a): State<App>) -> Result<StatusCode, ApiError> {
    a.led.off().await.map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn get_volume(State(a): State<App>) -> Result<Json<u8>, ApiError> {
    let value = a
        .led
        .api_json(Method::GET, "volume", None)
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(serde_json::from_value(value)?))
}
async fn put_volume(
    State(a): State<App>,
    Json(request): Json<VolumeRequest>,
) -> Result<StatusCode, ApiError> {
    a.led
        .api_json(
            Method::PUT,
            "volume",
            Some(serde_json::json!({"percent": request.percent})),
        )
        .await
        .map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn get_bluetooth_status(State(a): State<App>) -> Result<Json<serde_json::Value>, ApiError> {
    let value = a
        .led
        .api_json(Method::GET, "bluetooth/status", None)
        .await
        .map_err(ApiError::backend)?;
    Ok(Json(value))
}
async fn put_bluetooth_power(
    State(a): State<App>,
    Json(request): Json<BluetoothPowerRequest>,
) -> Result<StatusCode, ApiError> {
    a.led
        .api_json(
            Method::PUT,
            "bluetooth/power",
            Some(serde_json::json!({"on": request.on})),
        )
        .await
        .map_err(ApiError::backend)?;
    Ok(StatusCode::NO_CONTENT)
}
// Wi-Fi: wpa_cli and the supervisor block (processes, the settings lock
// taken from a plain thread), so they run off the async workers.
async fn wifi_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::backend(e.into()))?
        .map_err(wifi_operation_error)
}
fn wifi_operation_error(error: anyhow::Error) -> ApiError {
    if error.downcast_ref::<wifi::NetworkNotFound>().is_some() {
        ApiError::not_found(error)
    } else {
        ApiError::backend(error)
    }
}
async fn wifi_setup_request(wifi: Arc<wifi_setup::Setup>, on: bool) -> Result<(), ApiError> {
    task::spawn_blocking(move || wifi.request(on))
        .await
        .map_err(|error| ApiError::backend(error.into()))?
        .map_err(ApiError::new)
}
async fn get_wifi(State(a): State<App>) -> Result<Json<serde_json::Value>, ApiError> {
    let wifi = a.wifi.clone();
    Ok(Json(wifi_blocking(move || wifi.status()).await?))
}
async fn get_wifi_networks(State(a): State<App>) -> Result<Json<serde_json::Value>, ApiError> {
    let wifi = a.wifi.clone();
    let status = wifi_blocking(move || wifi.status()).await?;
    Ok(Json(serde_json::json!({
        "saved": status["saved"],
        "around": status["around"]
    })))
}
async fn get_wifi_setup(State(a): State<App>) -> Result<Json<serde_json::Value>, ApiError> {
    let wifi = a.wifi.clone();
    let status = wifi_blocking(move || wifi.status()).await?;
    Ok(Json(status["setup"].clone()))
}
async fn post_wifi_scan(State(a): State<App>) -> Result<Json<serde_json::Value>, ApiError> {
    let wifi = a.wifi.clone();
    let started = wifi_blocking(move || wifi.scan()).await?;
    Ok(Json(serde_json::json!({"started": started})))
}
async fn post_wifi_network(
    State(a): State<App>,
    Json(request): Json<wifi_setup::NetworkRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    wifi::check(&request.ssid, &request.password).map_err(ApiError::new)?;
    let wifi = a.wifi.clone();
    let leaving = wifi_blocking(move || wifi.add(&request.ssid, &request.password)).await?;
    Ok(Json(serde_json::json!({ "leaving": leaving })))
}
async fn delete_wifi_network(
    State(a): State<App>,
    AxumPath(ssid): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    let wifi = a.wifi.clone();
    wifi_blocking(move || wifi.forget(&ssid)).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn post_wifi_setup(
    State(a): State<App>,
    Json(request): Json<WifiSetupRequest>,
) -> Result<StatusCode, ApiError> {
    wifi_setup_request(a.wifi.clone(), request.on).await?;
    Ok(StatusCode::ACCEPTED)
}
#[derive(Debug)]
struct ApiError {
    error: anyhow::Error,
    status: StatusCode,
}
impl ApiError {
    fn new(error: anyhow::Error) -> Self {
        Self {
            error,
            status: StatusCode::BAD_REQUEST,
        }
    }

    fn backend(error: anyhow::Error) -> Self {
        Self {
            error,
            status: StatusCode::BAD_GATEWAY,
        }
    }

    fn not_found(error: anyhow::Error) -> Self {
        Self {
            error,
            status: StatusCode::NOT_FOUND,
        }
    }
}
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self::new(e.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({"error":self.error.to_string()})),
        )
            .into_response()
    }
}

/// Binary packages reach BetterYAMd on loopback unless it listens on one address only.
fn runtime_api_url(listen: &str) -> Result<String> {
    let mut address: std::net::SocketAddr = listen
        .parse()
        .with_context(|| format!("listen address {listen} must be IP:port"))?;
    if address.ip().is_unspecified() {
        address.set_ip(match address {
            std::net::SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
            std::net::SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
        });
    }
    Ok(format!("http://{address}"))
}

fn lan_ipv4_priority(address: Ipv4Addr) -> Option<u8> {
    // the setup access point's own address is not the LAN
    if address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || address == wifi_setup::AP_IP
    {
        return None;
    }
    if address.is_private() {
        Some(0)
    } else if address.is_link_local() {
        Some(2)
    } else {
        Some(1)
    }
}

fn discover_lan_ipv4() -> Result<Ipv4Addr> {
    let mut interfaces = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut interfaces) } != 0 {
        return Err(std::io::Error::last_os_error()).context("enumerate network interfaces");
    }

    let mut candidates = Vec::new();
    let mut interface = interfaces;
    while !interface.is_null() {
        let item = unsafe { &*interface };
        if !item.ifa_addr.is_null()
            && unsafe { (*item.ifa_addr).sa_family as i32 } == libc::AF_INET
            && item.ifa_flags & libc::IFF_UP as u32 != 0
            && item.ifa_flags & libc::IFF_LOOPBACK as u32 == 0
        {
            let sockaddr = unsafe { &*(item.ifa_addr as *const libc::sockaddr_in) };
            let address = Ipv4Addr::from(u32::from_be(sockaddr.sin_addr.s_addr));
            if let Some(priority) = lan_ipv4_priority(address) {
                let name = unsafe { CStr::from_ptr(item.ifa_name) }
                    .to_string_lossy()
                    .into_owned();
                candidates.push((priority, name, address));
            }
        }
        interface = item.ifa_next;
    }
    unsafe { libc::freeifaddrs(interfaces) };

    candidates.sort_unstable();
    candidates
        .first()
        .map(|(_, _, address)| *address)
        .context("no active LAN IPv4 address found; specify --listen IP:PORT")
}

/// The LAN address to serve on, waited for: a station that starts without a
/// network gets one later (the Wi-Fi setup access point is how, if enabled).
async fn wait_for_lan_ipv4() -> Ipv4Addr {
    let mut said = false;
    loop {
        match discover_lan_ipv4() {
            Ok(address) => return address,
            Err(error) if !said => {
                tracing::warn!(%error, "waiting for a LAN address");
                said = true;
            }
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();
    fs::create_dir_all(&args.data_dir)?;
    let settings_path = args.data_dir.join("settings.toml");
    // A broken settings.toml must not silently fall back to defaults that
    // re-enable the web API.
    let settings: Settings = match fs::read_to_string(&settings_path) {
        Ok(raw) => {
            toml::from_str(&raw).with_context(|| format!("invalid {}", settings_path.display()))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(error) => {
            return Err(error).with_context(|| format!("read {}", settings_path.display()))
        }
    };
    settings.wifi_setup.validate()?;
    let settings = Arc::new(Mutex::new(settings));
    // the Wi-Fi setup runs whatever the network does: without one it is
    // the way to get one
    let wifi = wifi_setup::Setup::new(args.data_dir.clone(), settings.clone());
    wifi.start(tokio::runtime::Handle::current());
    let listen = match args.listen {
        Some(listen) => listen,
        None => SocketAddr::new(wait_for_lan_ipv4().await.into(), 8099),
    };
    let led = Arc::new(Led::new()?);
    let mut device = detect_device(&args.data_dir, args.model);
    if device.id.is_empty() {
        device.id = device_identifier(&device.model);
    }
    apply_device_info(&mut device, led.device_info().await?);
    let wifi_available = wifi::available();
    for capability in ["wifi", "wifi.status", "wifi.scan", "wifi.networks"] {
        device
            .capabilities
            .insert(capability.into(), wifi_available);
        if wifi_available && !device.features.iter().any(|feature| feature == capability) {
            device.features.push(capability.into());
        }
    }
    let ap = {
        let wifi = wifi.clone();
        task::spawn_blocking(move || wifi.ap_available()).await?
    };
    device.capabilities.insert("wifi.ap".into(), ap);
    if ap && !device.features.iter().any(|f| f == "wifi.ap") {
        device.features.push("wifi.ap".into());
    }
    tracing::info!(model=%device.model,features=?device.features,capabilities=?device.capabilities,"detected device");
    let app = App {
        root: args.data_dir.clone(),
        settings,
        device: Arc::new(Mutex::new(device)),
        led,
        plugins: Arc::new(Mutex::new(PluginManager::new(
            args.data_dir.join("plugins"),
        )?)),
        runtime_tokens: Arc::new(Mutex::new(HashMap::new())),
        runtime_api_url: runtime_api_url(&listen.to_string())?,
        wifi,
    };
    let services = {
        let device = app.device.lock().await.clone();
        app.plugins
            .lock()
            .await
            .list(&device)?
            .into_iter()
            .filter(|package| {
                package.enabled && package.manifest.launch.mode == LaunchMode::Service
            })
            .map(|package| package.manifest.id)
            .collect::<Vec<_>>()
    };
    for id in services {
        if let Err(error) = start_package(app.clone(), id.clone(), Vec::new()).await {
            tracing::error!(package = %id, %error, "service package failed to start");
        }
    }
    let web = app.settings.lock().await.web.clone();
    if web.ui && !web.api {
        bail!("web.ui requires web.api")
    }
    if !web.ui && !web.api {
        tracing::info!("web interface is disabled");
        std::future::pending::<()>().await;
    }
    let mut router = Router::new();
    if web.ui {
        router = router.route("/", get(index));
    }
    if web.api {
        router = router
            .route("/api/plugins", get(list).post(install))
            .route("/api/plugins/registry", post(install_registry))
            .route("/api/plugins/:id", delete(remove))
            .route("/api/plugins/:id/toggle", post(toggle))
            .route("/api/plugins/:id/invoke", post(invoke))
            .route("/api/plugins/:id/stop", post(stop_plugin))
            .route("/api/plugins/:id/logs", get(package_logs))
            .route("/api/runtime/:id/device", get(runtime_device))
            .route("/api/runtime/:id/sensors", get(runtime_sensors))
            .route("/api/runtime/:id/sensors/:sensor", get(runtime_sensor_read))
            .route("/api/runtime/:id/audio/play", post(runtime_audio_play))
            .route("/api/runtime/:id/led", post(runtime_led))
            .route("/api/runtime/:id/led/off", post(runtime_led_off))
            .route("/api/runtime/:id/logo", post(runtime_logo))
            .route("/api/runtime/:id/logo/off", post(runtime_logo_off))
            .route("/api/runtime/:id/clock", post(runtime_clock))
            .route(
                "/api/runtime/:id/volume",
                get(runtime_volume_get).put(runtime_volume_set),
            )
            .route(
                "/api/runtime/:id/bluetooth/status",
                get(runtime_bluetooth_status),
            )
            .route(
                "/api/runtime/:id/bluetooth/power",
                put(runtime_bluetooth_power),
            )
            .route("/api/runtime/:id/input/devices", get(runtime_input_devices))
            .route(
                "/api/runtime/:id/input/next-event",
                post(runtime_input_next_event),
            )
            .route("/api/runtime/:id/wifi", get(runtime_wifi))
            .route("/api/runtime/:id/wifi/scan", post(runtime_wifi_scan))
            .route(
                "/api/runtime/:id/wifi/networks",
                get(runtime_wifi_networks).post(runtime_wifi_network),
            )
            .route(
                "/api/runtime/:id/wifi/networks/:ssid",
                delete(runtime_wifi_forget),
            )
            .route(
                "/api/runtime/:id/wifi/setup",
                get(runtime_wifi_setup_status).post(runtime_wifi_setup),
            )
            .route("/api/settings", get(get_settings).put(put_settings))
            .route("/api/device", get(get_device).put(put_device))
            .route("/api/sensors", get(get_sensors))
            .route("/api/sensors/:sensor", get(get_sensor))
            .route("/api/clock", get(get_clock).put(put_clock))
            .route("/api/led", post(led_set))
            .route("/api/led/off", post(led_off))
            .route("/api/volume", get(get_volume).put(put_volume))
            .route("/api/bluetooth/status", get(get_bluetooth_status))
            .route("/api/bluetooth/power", put(put_bluetooth_power))
            .route("/api/wifi", get(get_wifi))
            .route("/api/wifi/scan", post(post_wifi_scan))
            .route(
                "/api/wifi/networks",
                get(get_wifi_networks).post(post_wifi_network),
            )
            .route("/api/wifi/networks/:ssid", delete(delete_wifi_network))
            .route("/api/wifi/setup", get(get_wifi_setup).post(post_wifi_setup))
            .route("/api/logo", post(logo_set))
            .route("/api/logo/off", post(logo_off));
    }
    let router = router
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(app);
    let l = TcpListener::bind(listen).await?;
    tracing::info!("web UI: http://{}", listen);
    axum::serve(l, router).await?;
    Ok(())
}
const UI: &str = include_str!("web/index.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_address_selection_prefers_private_ipv4_addresses() {
        assert_eq!(lan_ipv4_priority(Ipv4Addr::new(192, 168, 1, 20)), Some(0));
        assert_eq!(lan_ipv4_priority(Ipv4Addr::new(8, 8, 8, 8)), Some(1));
        assert_eq!(lan_ipv4_priority(Ipv4Addr::new(169, 254, 1, 2)), Some(2));
        assert_eq!(lan_ipv4_priority(Ipv4Addr::LOCALHOST), None);
        assert_eq!(lan_ipv4_priority(Ipv4Addr::UNSPECIFIED), None);
    }

    #[test]
    fn network_policy_blocks_loopback() {
        let policy = NetworkPolicy {
            allow: vec!["*".into()],
            allow_insecure_http: true,
        };
        assert!(network_url_allowed(&policy, "http://127.0.0.1/").is_err());
    }

    #[test]
    fn input_scope_is_limited_to_declared_devices() {
        let manifest: Manifest = toml::from_str(
            "id='p'\nname='p'\nversion='1'\nentry='main.lua'\npermissions=['input']\n[input]\ndevices=['mute']\n",
        )
        .unwrap();
        assert!(validate_input_policy(&manifest).is_ok());
        assert_eq!(input_scope(&manifest).unwrap(), &["mute"]);
    }

    #[test]
    fn ipc_permissions_require_an_explicit_valid_service_allowlist() {
        let manifest = |permissions: &str, services: &str| {
            toml::from_str::<Manifest>(&format!(
                "id='p'\nname='p'\nversion='1'\nentry='main.lua'\npermissions={permissions}\n[ipc]\nservices={services}\n"
            ))
            .unwrap()
        };

        assert!(
            validate_ipc_policy(&manifest("['ipc.read', 'ipc.control']", "['mediad']")).is_ok()
        );
        assert!(validate_ipc_policy(&manifest("['ipc.read']", "[]")).is_err());
        assert!(validate_ipc_policy(&manifest("[]", "['mediad']")).is_err());
        assert!(validate_ipc_policy(&manifest("['ipc.control']", "['mediad', 'mediad']")).is_err());
        assert!(validate_ipc_policy(&manifest("['ipc.read']", "['MediaD']")).is_err());
    }

    #[test]
    fn binary_input_requests_cannot_escape_declared_device_scope() {
        let allowed = vec!["adc_keypad".to_string()];
        assert!(validate_input_scope(&allowed, &allowed).is_ok());
        assert!(validate_input_scope(&["other".into()], &allowed).is_err());
        assert!(validate_input_scope(&[], &allowed).is_err());
    }

    #[test]
    fn device_api_profile_drives_capabilities_and_led_count() {
        let mut device = DeviceProfile::default();
        apply_device_info(
            &mut device,
            DeviceInfo {
                id: "other-device".into(),
                model: "Other Device".into(),
                architecture: "aarch64".into(),
                led_count: 12,
                features: vec!["led.ring.rgb".into(), "led.ring.12".into()],
                capabilities: HashMap::new(),
            },
        );
        assert_eq!(device.id, "other-device");
        assert_eq!(device.led_count, 12);
        assert_eq!(
            device.features,
            [
                "led.ring.rgb",
                "led.ring.12",
                "runtime.lua.v1",
                "plugin.yap.v1"
            ]
        );
        assert_eq!(device.capabilities.get("led.ring.rgb"), Some(&true));
        assert_eq!(device.capabilities.get("clock.segments"), Some(&false));
    }

    #[test]
    fn strips_only_a_separated_four_character_uppercase_device_id() {
        assert_eq!(
            strip_device_id("Yandex-Station-Midi-44QK"),
            "Yandex-Station-Midi"
        );
        assert_eq!(strip_device_id("Yandex-Mini3-J3MK"), "Yandex-Mini3");
        assert_eq!(
            strip_device_id("YandexStationMini_729E"),
            "YandexStationMini"
        );
        assert_eq!(strip_device_id("Yandex-Lite-gen2"), "Yandex-Lite-gen2");
        assert_eq!(
            strip_device_id("yandex-station-mini-44qk"),
            "yandex-station-mini-44qk"
        );
        assert_eq!(strip_device_id("YandexMini"), "YandexMini");
    }

    #[test]
    fn explicit_capability_false_overrides_matching_feature() {
        let mut device = DeviceProfile {
            features: vec!["clock.segments".into()],
            capabilities: HashMap::from([("clock.segments".into(), false)]),
            ..DeviceProfile::default()
        };
        assert!(!capability_available(&device, "clock.segments"));
        device.capabilities.remove("clock.segments");
        assert!(capability_available(&device, "clock.segments"));
    }

    #[test]
    fn lua_is_available_returns_the_profile_boolean() {
        let lua = Lua::new();
        let device = DeviceProfile {
            capabilities: HashMap::from([("clock.segments".into(), false)]),
            ..DeviceProfile::default()
        };
        let segments = lua.create_table().unwrap();
        set_lua_availability(&lua, &segments, &device, "clock.segments").unwrap();
        let metatable = lua.create_table().unwrap();
        metatable
            .set(
                "__call",
                lua.create_function(|_, (_segments, levels): (mlua::Table, mlua::Table)| {
                    Ok(levels.pairs::<String, u8>().count())
                })
                .unwrap(),
            )
            .unwrap();
        segments.set_metatable(Some(metatable));
        lua.globals().set("segments", segments).unwrap();
        let (available, segment_count) = lua
            .load("return segments.is_available(), segments({left=1,right=2})")
            .eval::<(bool, usize)>()
            .unwrap();
        assert!(!available);
        assert_eq!(segment_count, 2);
    }

    #[test]
    fn device_profile_reports_boolean_capabilities() {
        let mut profile = DeviceProfile {
            features: vec!["led.ring.rgb".into()],
            ..DeviceProfile::default()
        };
        populate_capabilities(&mut profile);
        let value = serde_json::to_value(profile).unwrap();
        assert_eq!(value["capabilities"]["led.ring.rgb"], true);
        assert_eq!(value["capabilities"]["clock.segments"], false);
    }

    #[test]
    fn binary_packages_are_exposed_as_shell_commands_and_removed_cleanly() {
        let temporary = std::env::temp_dir().join(format!("betteryam-path-{}", Uuid::new_v4()));
        let plugins = temporary.join("plugins");
        let package = plugins.join("nano");
        fs::create_dir_all(package.join("nano/bin")).unwrap();
        fs::create_dir_all(package.join("nano/lib")).unwrap();
        fs::write(
            package.join("nano/bin/nano"),
            b"#!/bin/sh\nprintf '<%s>\\n' \"$@\"\n",
        )
        .unwrap();
        set_executable(&package.join("nano/bin/nano")).unwrap();
        fs::write(
            package.join("plugin.toml"),
            r#"
id = "nano"
name = "Nano"
version = "1.0.0"
entry = "launch.lua"
permissions = ["exec"]

[launch]
kind = "binary"
path = "nano/bin/nano"
args = ["--flag with space"]
"#,
        )
        .unwrap();

        let commands = temporary.join("bin");
        let profile = temporary.join("profile.d/betteryam.sh");
        let mut manager =
            PluginManager::with_paths(plugins.clone(), commands.clone(), profile.clone()).unwrap();
        let wrapper = fs::read_to_string(commands.join("nano")).unwrap();
        assert!(wrapper.starts_with(COMMAND_WRAPPER_MARKER));
        assert!(wrapper.contains("nano/bin/nano"));
        assert!(wrapper.contains("'--flag with space' \"$@\""));
        assert!(wrapper.contains("if [ -n \"${LD_LIBRARY_PATH:-}\" ]; then"));
        assert!(fs::read_to_string(&profile)
            .unwrap()
            .contains(&commands.display().to_string()));

        let output = std::process::Command::new(commands.join("nano"))
            .arg("user arg")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "<--flag with space>\n<user arg>\n"
        );

        let path_output = std::process::Command::new("sh")
            .args([
                "-c",
                ". \"$1\" && case \":$PATH:\" in *:\"$2\":*) printf included;; *) exit 1;; esac",
                "sh",
            ])
            .arg(&profile)
            .arg(&commands)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(path_output.status.success());
        assert_eq!(String::from_utf8(path_output.stdout).unwrap(), "included");

        manager.remove("nano").unwrap();
        assert!(!commands.join("nano").exists());
        assert!(!profile.exists());
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn binary_command_sync_does_not_replace_unmanaged_commands() {
        let temporary =
            std::env::temp_dir().join(format!("betteryam-path-collision-{}", Uuid::new_v4()));
        let plugins = temporary.join("plugins");
        let package = plugins.join("nano");
        fs::create_dir_all(package.join("nano/bin")).unwrap();
        fs::write(package.join("nano/bin/nano"), b"binary").unwrap();
        fs::write(
            package.join("plugin.toml"),
            r#"
id = "nano"
name = "Nano"
version = "1.0.0"
entry = "launch.lua"
permissions = ["exec"]

[launch]
kind = "binary"
path = "nano/bin/nano"
"#,
        )
        .unwrap();
        let commands = temporary.join("bin");
        fs::create_dir_all(&commands).unwrap();
        fs::write(commands.join("nano"), "user-owned command").unwrap();

        let result = PluginManager::with_paths(
            plugins,
            commands.clone(),
            temporary.join("profile.d/betteryam.sh"),
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(commands.join("nano")).unwrap(),
            "user-owned command"
        );
        fs::remove_dir_all(temporary).unwrap();
    }
}
