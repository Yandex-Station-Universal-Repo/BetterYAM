use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
};

#[cfg(target_os = "linux")]
use serde_json::json;
#[cfg(target_os = "linux")]
use std::fs::File;

#[derive(Debug, Deserialize)]
struct NextEventRequest {
    devices: Vec<String>,
    timeout_ms: u64,
}

impl NextEventRequest {
    fn validate(&self) -> Result<()> {
        if self.timeout_ms > 60_000 {
            bail!("input timeout is limited to 60 seconds")
        }
        if self.devices.is_empty() {
            bail!("input event request must name at least one device")
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxInputEvent {
    seconds: libc::c_long,
    microseconds: libc::c_long,
    event_type: u16,
    code: u16,
    value: i32,
}

#[derive(Debug)]
struct EventDevice {
    name: String,
    path: PathBuf,
}

fn event_devices() -> Result<Vec<EventDevice>> {
    let input_class = PathBuf::from("/sys/class/input");
    let entries = match fs::read_dir(&input_class) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", input_class.display())),
    };

    let mut devices = Vec::new();
    for entry in entries {
        let entry = entry.context("read Linux input device entry")?;
        let event_name = entry.file_name();
        if !event_name.to_string_lossy().starts_with("event") {
            continue;
        }
        let name_path = entry.path().join("device/name");
        let name = match fs::read_to_string(&name_path) {
            Ok(name) => name.trim().to_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", name_path.display()))
            }
        };
        if name.is_empty() {
            continue;
        }
        devices.push(EventDevice {
            name,
            path: PathBuf::from("/dev/input").join(event_name),
        });
    }
    devices.sort_by(|left, right| (&left.name, &left.path).cmp(&(&right.name, &right.path)));
    Ok(devices)
}

pub async fn device_names() -> Result<Vec<String>> {
    tokio::task::spawn_blocking(|| {
        let mut names = event_devices()?
            .into_iter()
            .filter_map(|device| match OpenOptions::new().read(true).open(&device.path) {
                Ok(_) => Some(Ok(device.name)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    tracing::warn!(path=%device.path.display(), %error, "input device is not accessible");
                    None
                }
                Err(error) => Some(Err(error).with_context(|| {
                    format!("open input device {}", device.path.display())
                })),
            })
            .collect::<Result<Vec<_>>>()?;
        names.sort();
        names.dedup();
        Ok(names)
    })
    .await
    .context("input device discovery task failed")?
}

pub async fn next_event(body: Option<Value>) -> Result<Value> {
    let request: NextEventRequest =
        serde_json::from_value(body.context("input event request body is required")?)
            .context("invalid input event request")?;
    request.validate()?;

    tokio::task::spawn_blocking(move || next_event_blocking(request))
        .await
        .context("input event task failed")?
}

#[cfg(target_os = "linux")]
fn next_event_blocking(request: NextEventRequest) -> Result<Value> {
    let requested: std::collections::HashSet<_> = request.devices.iter().cloned().collect();
    let devices: Vec<_> = event_devices()?
        .into_iter()
        .filter(|device| requested.contains(&device.name))
        .collect();
    if devices.is_empty() {
        bail!("none of the requested input devices are available")
    }

    let files: Vec<(String, File)> = devices
        .into_iter()
        .map(|device| {
            OpenOptions::new()
                .read(true)
                .open(&device.path)
                .with_context(|| format!("open input device {}", device.path.display()))
                .map(|file| (device.name, file))
        })
        .collect::<Result<_>>()?;
    let mut descriptors: Vec<_> = files
        .iter()
        .map(|(_, file)| libc::pollfd {
            fd: std::os::fd::AsRawFd::as_raw_fd(file),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();

    let timeout = libc::c_int::try_from(request.timeout_ms).context("invalid input timeout")?;
    let result = unsafe {
        libc::poll(
            descriptors.as_mut_ptr(),
            descriptors.len() as libc::nfds_t,
            timeout,
        )
    };
    if result == 0 {
        return Ok(Value::Null);
    }
    if result < 0 {
        return Err(std::io::Error::last_os_error()).context("wait for Linux input event");
    }

    for (index, descriptor) in descriptors.iter().enumerate() {
        if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            bail!(
                "input device {} closed or returned an error",
                files[index].0
            )
        }
        if descriptor.revents & libc::POLLIN == 0 {
            continue;
        }
        let mut event = std::mem::MaybeUninit::<LinuxInputEvent>::uninit();
        let bytes = unsafe {
            libc::read(
                descriptor.fd,
                event.as_mut_ptr().cast(),
                std::mem::size_of::<LinuxInputEvent>(),
            )
        };
        if bytes < 0 {
            return Err(std::io::Error::last_os_error()).context("read Linux input event");
        }
        if bytes as usize != std::mem::size_of::<LinuxInputEvent>() {
            bail!("short read from Linux input device")
        }
        let event = unsafe { event.assume_init() };
        let time_ms = event
            .seconds
            .saturating_mul(1000)
            .saturating_add(event.microseconds / 1000);
        return Ok(json!({
            "device": files[index].0,
            "type": event.event_type,
            "code": event.code,
            "value": event.value,
            "time_ms": time_ms,
        }));
    }
    Ok(Value::Null)
}

#[cfg(not(target_os = "linux"))]
fn next_event_blocking(_request: NextEventRequest) -> Result<Value> {
    bail!("input events are supported only on Linux")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn input_request_requires_devices_and_bounded_timeout() {
        let empty: NextEventRequest = serde_json::from_value(json!({
            "devices": [],
            "timeout_ms": 100
        }))
        .unwrap();
        assert!(empty.validate().is_err());

        let excessive: NextEventRequest = serde_json::from_value(json!({
            "devices": ["adc_keypad"],
            "timeout_ms": 60_001
        }))
        .unwrap();
        assert!(excessive.validate().is_err());
    }
}
