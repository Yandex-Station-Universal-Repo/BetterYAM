use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

const INPUT_CLASS: &str = "/sys/class/input";
const INPUT_DEVICE_DIR: &str = "/dev/input";
const ABS_DISTANCE: u32 = 0x19;
const IIO_ROOT: &str = "/sys/bus/iio/devices";
const LIGHT_ATTRIBUTES: [&str; 2] = ["in_illuminance_input", "in_intensity_input"];

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

struct LightSensor {
    id: String,
    input: PathBuf,
}

enum Sensor {
    Distance(DistanceSensor),
    Light(LightSensor),
}

impl Sensor {
    fn id(&self) -> &str {
        match self {
            Self::Distance(sensor) => &sensor.id,
            Self::Light(sensor) => &sensor.id,
        }
    }
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

fn light_sensors_in(root: &Path) -> Result<Vec<LightSensor>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };
    let mut sensors = Vec::new();
    for entry in entries {
        let device = entry.context("read IIO device entry")?.path();
        let Ok(name) = fs::read_to_string(device.join("name")) else {
            continue;
        };
        let name = name.trim();
        let Some(input) = LIGHT_ATTRIBUTES
            .iter()
            .map(|attribute| device.join(attribute))
            .find(|path| path.is_file())
        else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        sensors.push(LightSensor {
            id: sensor_id(name),
            input,
        });
    }
    sensors.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(sensors)
}

fn parse_lux(raw: &str) -> Option<f64> {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn sensors(ambient_light: bool) -> Result<Vec<Sensor>> {
    let mut sensors: Vec<_> = distance_sensors()?
        .into_iter()
        .map(Sensor::Distance)
        .collect();
    if ambient_light {
        sensors.extend(
            light_sensors_in(Path::new(IIO_ROOT))?
                .into_iter()
                .map(Sensor::Light),
        );
    }
    Ok(sensors)
}

fn reading(sensor: Sensor) -> Result<SensorInfo> {
    match sensor {
        Sensor::Distance(sensor) => {
            let value = read_distance(&sensor.path)?;
            Ok(SensorInfo {
                id: sensor.id,
                kind: "proximity".into(),
                unit: "mm".into(),
                value: sensor_value(value.value),
            })
        }
        Sensor::Light(sensor) => {
            let raw = fs::read_to_string(&sensor.input)
                .with_context(|| format!("read ambient light sensor {}", sensor.input.display()))?;
            Ok(SensorInfo {
                id: sensor.id,
                kind: "ambient_light".into(),
                unit: "lux".into(),
                value: parse_lux(&raw),
            })
        }
    }
}

pub fn list(ambient_light: bool) -> Result<Vec<SensorInfo>> {
    sensors(ambient_light)?.into_iter().map(reading).collect()
}

pub fn read(id: &str, ambient_light: bool) -> Result<SensorInfo> {
    let sensor = sensors(ambient_light)?
        .into_iter()
        .find(|sensor| sensor.id() == id)
        .with_context(|| format!("sensor '{id}' is not available"))?;
    reading(sensor)
}

fn sensor_value(value: i32) -> Option<f64> {
    // Some ABS_DISTANCE drivers report calibration bounds that exclude live readings.
    (0..=MAX_DISTANCE_MM)
        .contains(&value)
        .then_some(f64::from(value))
}

const MAX_DISTANCE_MM: i32 = 8192;

pub async fn available(ambient_light: bool) -> bool {
    match tokio::task::spawn_blocking(move || {
        sensors(ambient_light).map(|sensors| !sensors.is_empty())
    })
    .await
    {
        Ok(Ok(available)) => available,
        Ok(Err(error)) => {
            tracing::warn!(%error, "sensor discovery failed");
            false
        }
        Err(error) => {
            tracing::warn!(%error, "sensor discovery task failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_distance_ioctl_has_linux_read_request_shape() {
        let request = abs_distance_request() as u32;
        assert_eq!(request >> 30, 2);
        assert_eq!((request >> 8) & 0xff, b'E' as u32);
        assert_eq!(request & 0xff, 0x40 + ABS_DISTANCE);
        assert_eq!(
            (request >> 16) & 0x3fff,
            std::mem::size_of::<InputAbsInfo>() as u32
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
    fn finds_iio_light_sensors_only() {
        let root = std::env::temp_dir().join(format!("byam-iio-{}", uuid::Uuid::new_v4()));
        let light = root.join("iio:device1");
        let adc = root.join("iio:device0");
        fs::create_dir_all(&light).unwrap();
        fs::create_dir_all(&adc).unwrap();
        fs::write(light.join("name"), "ltr308als01\n").unwrap();
        fs::write(light.join("in_intensity_input"), "4.200000\n").unwrap();
        fs::write(adc.join("name"), "meson-axg-saradc\n").unwrap();
        fs::write(adc.join("in_voltage0_input"), "1800\n").unwrap();
        let mut sensors = light_sensors_in(&root).unwrap();
        assert_eq!(sensors.len(), 1);
        assert_eq!(sensors[0].id, "ltr308als01");
        let info = reading(Sensor::Light(sensors.remove(0))).unwrap();
        assert_eq!(info.kind, "ambient_light");
        assert_eq!(info.unit, "lux");
        assert_eq!(info.value, Some(4.2));
        fs::remove_dir_all(&root).unwrap();
        assert!(light_sensors_in(&root).unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_light_values() {
        assert_eq!(parse_lux(" 12.5\n"), Some(12.5));
        assert_eq!(parse_lux("-1"), None);
        assert_eq!(parse_lux("nan"), None);
        assert_eq!(parse_lux("x"), None);
    }

    #[test]
    fn distance_values_ignore_inconsistent_calibration_bounds() {
        assert_eq!(sensor_value(819), Some(819.0));
        assert_eq!(sensor_value(-1), None);
        assert_eq!(sensor_value(MAX_DISTANCE_MM + 1), None);
    }
}
