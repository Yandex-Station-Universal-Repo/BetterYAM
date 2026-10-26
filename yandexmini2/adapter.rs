use crate::clock::Clock;
use anyhow::{bail, Context, Result};
use betteryam_runtime::device::{
    AdapterProfile, ClockBackend, ColorOrder, DeviceAdapter, LedBackend,
};
use std::{fs, path::Path};

const LED_COUNT: usize = 6;
const CHANNEL_COUNT: usize = LED_COUNT * 3;

const LED_PATHS: [&str; CHANNEL_COUNT] = [
    "/sys/class/leds/mini2:red:ring1/brightness",
    "/sys/class/leds/mini2:green:ring1/brightness",
    "/sys/class/leds/mini2:blue:ring1/brightness",
    "/sys/class/leds/mini2:red:ring2/brightness",
    "/sys/class/leds/mini2:green:ring2/brightness",
    "/sys/class/leds/mini2:blue:ring2/brightness",
    "/sys/class/leds/mini2:red:ring3/brightness",
    "/sys/class/leds/mini2:green:ring3/brightness",
    "/sys/class/leds/mini2:blue:ring3/brightness",
    "/sys/class/leds/mini2:red:ring4/brightness",
    "/sys/class/leds/mini2:green:ring4/brightness",
    "/sys/class/leds/mini2:blue:ring4/brightness",
    "/sys/class/leds/mini2:red:ring5/brightness",
    "/sys/class/leds/mini2:green:ring5/brightness",
    "/sys/class/leds/mini2:blue:ring5/brightness",
    "/sys/class/leds/mini2:red:ring6/brightness",
    "/sys/class/leds/mini2:green:ring6/brightness",
    "/sys/class/leds/mini2:blue:ring6/brightness",
];

pub struct YandexMini2;

impl DeviceAdapter for YandexMini2 {
    fn profile(&self) -> AdapterProfile {
        AdapterProfile {
            id: "yandexmini2".into(),
            model: "yandexmini2".into(),
            led_count: LED_COUNT,
            color_order: ColorOrder::Rgb,
            sensors_proximity: false,
            sensors_ambient_light: true,
        }
    }

    fn create_led_backend(&self) -> Result<Box<dyn LedBackend>> {
        if let Some(path) = LED_PATHS.iter().find(|path| !Path::new(path).exists()) {
            bail!("Yandex Mini 2 LED sysfs node is missing: {path}");
        }
        Ok(Box::new(Driver))
    }

    fn create_clock_backend(&self) -> Option<Box<dyn ClockBackend>> {
        Clock::detect().map(|clock| Box::new(clock) as Box<dyn ClockBackend>)
    }
}

struct Driver;

impl LedBackend for Driver {
    fn show(&self, frame: &[u8]) -> Result<()> {
        if frame.len() != CHANNEL_COUNT {
            bail!("unexpected Yandex Mini 2 LED frame length");
        }
        for (path, value) in LED_PATHS.iter().zip(frame) {
            fs::write(path, value.to_string())
                .with_context(|| format!("write Yandex Mini 2 LED node {path}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_describes_mini2_hardware() {
        let profile = YandexMini2.profile();
        assert_eq!(profile.id, "yandexmini2");
        assert_eq!(profile.led_count, LED_COUNT);
        assert!(matches!(profile.color_order, ColorOrder::Rgb));
        assert!(!profile.sensors_proximity);
        assert!(profile.sensors_ambient_light);
    }
}
