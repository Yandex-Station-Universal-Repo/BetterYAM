use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const POWER_SCRIPT: &str = "/system/vendor/quasar/bsa_server_run.sh";
const RFKILL_ROOT: &str = "/sys/class/rfkill";

#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub status: bool,
    pub power: bool,
    pub power_reason: Option<&'static str>,
}

fn power_control(script_available: bool, quasar_alive: bool) -> (bool, Option<&'static str>) {
    if quasar_alive {
        (false, Some("quasar running, not verified"))
    } else if !script_available {
        (false, Some("Bluetooth power control script is unavailable"))
    } else {
        (true, None)
    }
}

#[derive(Debug)]
struct Radio {
    state_path: PathBuf,
    powered: bool,
}

fn radio() -> Result<Option<Radio>> {
    let root = Path::new(RFKILL_ROOT);
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };

    let mut radios = Vec::new();
    for entry in entries {
        let entry = entry.context("read rfkill entry")?;
        let path = entry.path();
        let kind = match fs::read_to_string(path.join("type")) {
            Ok(kind) => kind,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("read {}/type", path.display()))
            }
        };
        if kind.trim() != "bluetooth" {
            continue;
        }
        let state_path = path.join("state");
        let state = fs::read_to_string(&state_path)
            .with_context(|| format!("read {}", state_path.display()))?;
        let powered = match state.trim() {
            "0" => false,
            "1" => true,
            value => bail!("invalid Bluetooth rfkill state {value}"),
        };
        radios.push(Radio {
            state_path,
            powered,
        });
    }
    radios.sort_by(|left, right| left.state_path.cmp(&right.state_path));
    if radios.is_empty() {
        return Ok(None);
    }
    // Yandex Station Lite may have both platform gpio rfkill (bt-dev) and hci rfkill.
    let any_powered = radios.iter().any(|r| r.powered);
    let primary = radios.into_iter().next().unwrap();
    Ok(Some(Radio {
        state_path: primary.state_path,
        powered: any_powered,
    }))
}

fn process_running(name: &str, required_argument: Option<&str>) -> Result<bool> {
    for entry in fs::read_dir("/proc").context("read /proc for Bluetooth process status")? {
        let entry = entry.context("read /proc entry")?;
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let cmdline_path = entry.path().join("cmdline");
        let cmdline = match fs::read(&cmdline_path) {
            Ok(cmdline) => cmdline,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) =>
            {
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", cmdline_path.display()))
            }
        };
        let arguments: Vec<_> = cmdline.split(|byte| *byte == 0).collect();
        let Some(executable) = arguments.first() else {
            continue;
        };
        let executable = std::str::from_utf8(executable).unwrap_or("");
        if Path::new(executable)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(name)
        {
            continue;
        }
        if match required_argument {
            Some(required) => arguments.iter().any(|arg| *arg == required.as_bytes()),
            None => true,
        } {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn capabilities() -> Result<Capabilities> {
    let quasar_alive =
        process_running("quasar_launcher2", None)? || process_running("maind", None)?;
    let script_available = Path::new(POWER_SCRIPT).is_file();
    let (power, power_reason) = power_control(script_available, quasar_alive);
    Ok(Capabilities {
        status: true,
        power,
        power_reason,
    })
}

fn status() -> Result<Value> {
    let radio = radio()?.context("Bluetooth rfkill device is unavailable")?;
    let capabilities = capabilities()?;
    Ok(json!({
        "powered": radio.powered,
        "server_running": process_running("bsa_server", None)?,
        "power_control_available": capabilities.power,
        "power_control_reason": capabilities.power_reason,
    }))
}

fn set_power(on: bool) -> Result<()> {
    let capabilities = capabilities()?;
    if let Some(reason) = capabilities.power_reason {
        bail!("{reason}")
    }
    let output = Command::new(POWER_SCRIPT)
        .arg(if on { "1" } else { "0" })
        .output()
        .context("run the station Bluetooth power control script")?;
    if !output.status.success() {
        bail!(
            "Bluetooth power control failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
    Ok(())
}

pub async fn availability() -> Result<Capabilities> {
    tokio::task::spawn_blocking(capabilities)
        .await
        .context("Bluetooth capability detection task failed")?
}

pub async fn api_json(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
    match (method.as_str(), path) {
        ("GET", "bluetooth/status") => tokio::task::spawn_blocking(status)
            .await
            .context("Bluetooth status task failed")?,
        ("PUT", "bluetooth/power") => {
            let on = body
                .as_ref()
                .and_then(|value| value.get("on"))
                .and_then(Value::as_bool)
                .context("Bluetooth power request requires boolean on")?;
            tokio::task::spawn_blocking(move || set_power(on))
                .await
                .context("Bluetooth power task failed")??;
            Ok(Value::Null)
        }
        _ => bail!("Yandex Mini does not support Bluetooth operation {method} /{path}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_power_requires_script_and_stopped_quasar() {
        assert_eq!(power_control(true, false), (true, None));
        assert_eq!(
            power_control(true, true),
            (false, Some("quasar running, not verified"))
        );
        assert!(!power_control(false, false).0);
    }
}
