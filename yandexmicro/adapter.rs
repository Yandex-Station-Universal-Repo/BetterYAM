use anyhow::{bail, Context, Result};
use betteryam_runtime::device::{AdapterProfile, ColorOrder, DeviceAdapter, LedBackend};
use std::{fs, path::Path};

const LED_COUNT: usize = 3;
const CHANNEL_COUNT: usize = LED_COUNT * 3;

const LED_PATHS: [&str; CHANNEL_COUNT] = [
    "/sys/class/leds/micro:red:ring1/brightness",
    "/sys/class/leds/micro:green:ring1/brightness",
    "/sys/class/leds/micro:blue:ring1/brightness",
    "/sys/class/leds/micro:red:ring2/brightness",
    "/sys/class/leds/micro:green:ring2/brightness",
    "/sys/class/leds/micro:blue:ring2/brightness",
    "/sys/class/leds/micro:red:ring3/brightness",
    "/sys/class/leds/micro:green:ring3/brightness",
    "/sys/class/leds/micro:blue:ring3/brightness",
];

pub struct YandexMicro;

impl DeviceAdapter for YandexMicro {
    fn profile(&self) -> AdapterProfile {
        AdapterProfile {
            id: "yandexmicro".into(),
            model: "yandexmicro".into(),
            led_count: LED_COUNT,
            color_order: ColorOrder::Rgb,
            sensors_proximity: false,
            sensors_ambient_light: false,
        }
    }

    fn create_led_backend(&self) -> Result<Box<dyn LedBackend>> {
        if let Some(path) = LED_PATHS.iter().find(|path| !Path::new(path).exists()) {
            bail!("Yandex Lite LED sysfs node is missing: {path}");
        }
        Ok(Box::new(Driver))
    }
}

struct Driver;

impl LedBackend for Driver {
    fn show(&self, frame: &[u8]) -> Result<()> {
        if frame.len() != CHANNEL_COUNT {
            bail!("unexpected Yandex Lite LED frame length");
        }
        for (path, value) in LED_PATHS.iter().zip(frame) {
            fs::write(path, value.to_string())
                .with_context(|| format!("write Yandex Lite LED node {path}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_describes_lite_led_and_sensor_hardware() {
        let profile = YandexMicro.profile();
        assert_eq!(profile.led_count, LED_COUNT);
        assert!(matches!(profile.color_order, ColorOrder::Rgb));
        assert!(!profile.sensors_proximity);
    }
}
