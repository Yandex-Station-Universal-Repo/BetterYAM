use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    os::fd::AsRawFd,
    path::PathBuf,
};

const INPUT_CLASS: &str = "/sys/class/input";
const INPUT_DEVICE_DIR: &str = "/dev/input";
const ABS_DISTANCE: u32 = 0x19;

#[cfg(any(target_os = "linux", target_os = "android"))]
type IoctlRequest = libc::c_int;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
type IoctlRequest = libc::c_ulong;

#[repr(C)]
#[derive(Default)]
struct InputAbsInfo {
    value: i32,
    _minimum: i32,
    _maximum: i32,
    _fuzz: i32,
    _flat: i32,
    _resolution: i32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SensorInfo {
    pub id: String,
    pub kind: String,
    pub unit: String,
    pub value: Option<f64>,
}

struct DistanceSensor {
    id: String,
    path: PathBuf,
}

fn abs_distance_request() -> IoctlRequest {
    const IOC_READ: u32 = 2;
    let request = (IOC_READ << 30)
        | ((std::mem::size_of::<InputAbsInfo>() as u32) << 16)
        | ((b'E' as u32) << 8)
        | (0x40 + ABS_DISTANCE);
    request as IoctlRequest
}

fn read_distance(path: &PathBuf) -> Result<InputAbsInfo> {
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("open proximity sensor {}", path.display()))?;
    let mut info = InputAbsInfo::default();
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            abs_distance_request(),
            &mut info as *mut InputAbsInfo,
        )
    };
    if result < 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("read proximity sensor {}", path.display()));
    }
    Ok(info)
}

fn sensor_id(name: &str) -> String {
    let mut id = String::with_capacity(name.len());
    let mut separator = false;
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() {
            if separator && !id.is_empty() {
                id.push('-');
            }
            id.push((byte as char).to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    id
}

fn is_unsupported_ioctl(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.raw_os_error(),
                Some(libc::ENOTTY) | Some(libc::EINVAL) | Some(libc::EOPNOTSUPP)
            )
        })
    })
}

fn is_unavailable_device(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            )
        })
    })
}

fn distance_sensors() -> Result<Vec<DistanceSensor>> {
    let entries = match fs::read_dir(INPUT_CLASS) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {INPUT_CLASS}")),
    };

    let mut sensors = Vec::new();
    for entry in entries {
        let entry = entry.context("read Linux input class entry")?;
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
        let path = PathBuf::from(INPUT_DEVICE_DIR).join(event_name);
        match read_distance(&path) {
            Ok(_) => sensors.push(DistanceSensor {
                id: sensor_id(&name),
                path,
            }),
            Err(error) if is_unsupported_ioctl(&error) => continue,
            Err(error) if is_unavailable_device(&error) => {
                tracing::debug!(%error, "skipping inaccessible or non-distance input device");
            }
            Err(error) => return Err(error),
        }
    }
    sensors.sort_by(|left, right| left.path.cmp(&right.path));
    sensors.dedup_by(|left, right| left.path == right.path);
    Ok(sensors)
}

pub fn list() -> Result<Vec<SensorInfo>> {
    distance_sensors()?
        .into_iter()
        .map(|sensor| {
            let value = read_distance(&sensor.path)?;
            Ok(SensorInfo {
                id: sensor.id,
                kind: "proximity".into(),
                unit: "mm".into(),
                value: sensor_value(value.value),
            })
        })
        .collect()
}

pub fn read(id: &str) -> Result<SensorInfo> {
    let sensor = distance_sensors()?
        .into_iter()
        .find(|sensor| sensor.id == id)
        .with_context(|| format!("sensor '{id}' is not available"))?;
    let value = read_distance(&sensor.path)?;
    Ok(SensorInfo {
        id: sensor.id,
        kind: "proximity".into(),
        unit: "mm".into(),
        value: sensor_value(value.value),
    })
}

fn sensor_value(value: i32) -> Option<f64> {
    // Some ABS_DISTANCE drivers report calibration bounds that exclude live readings.
    (0..=MAX_DISTANCE_MM)
        .contains(&value)
        .then_some(f64::from(value))
}

const MAX_DISTANCE_MM: i32 = 8192;

pub async fn available() -> bool {
    match tokio::task::spawn_blocking(|| distance_sensors().map(|sensors| !sensors.is_empty()))
        .await
    {
        Ok(Ok(available)) => available,
        Ok(Err(error)) => {
            tracing::warn!(%error, "proximity sensor discovery failed");
            false
        }
        Err(error) => {
            tracing::warn!(%error, "proximity sensor discovery task failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_distance_ioctl_has_linux_read_request_shape() {
        assert_eq!(abs_distance_request() >> 30, 2);
        assert_eq!((abs_distance_request() >> 8) & 0xff, b'E' as IoctlRequest);
        assert_eq!(
            abs_distance_request() & 0xff,
            (0x40 + ABS_DISTANCE) as IoctlRequest
        );
        assert_eq!(
            (abs_distance_request() >> 16) & 0x3fff,
            std::mem::size_of::<InputAbsInfo>() as IoctlRequest
        );
    }

    #[test]
    fn sensor_ids_are_stable_path_segments() {
        assert_eq!(
            sensor_id("STM VL53L0 proximity sensor"),
            "stm-vl53l0-proximity-sensor"
        );
        assert_eq!(sensor_id("  "), "");
    }

    #[test]
    fn distance_values_ignore_inconsistent_calibration_bounds() {
        assert_eq!(sensor_value(819), Some(819.0));
        assert_eq!(sensor_value(-1), None);
        assert_eq!(sensor_value(MAX_DISTANCE_MM + 1), None);
    }
}
