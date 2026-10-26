use crate::clock::Clock;
use anyhow::{bail, Context, Result};
use betteryam_runtime::device::{
    AdapterProfile, ClockBackend, ColorOrder, DeviceAdapter, LedBackend,
};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::FileExt,
    path::Path,
};

const LED_COUNT: usize = 6;
const CHANNEL_COUNT: usize = LED_COUNT * 3;
const FRAMEBUFFER: &str = "/dev/fb0";
const FRAMEBUFFER_SYSFS: &str = "/sys/class/graphics/fb0";

pub struct YandexMini3;

impl DeviceAdapter for YandexMini3 {
    fn profile(&self) -> AdapterProfile {
        AdapterProfile {
            id: "yandexmini3".into(),
            model: String::new(),
            led_count: LED_COUNT,
            color_order: ColorOrder::Bgr,
            sensors_proximity: false,
            sensors_ambient_light: true,
        }
    }

    fn create_led_backend(&self) -> Result<Box<dyn LedBackend>> {
        Ok(Box::new(Ring::open()?))
    }

    fn create_clock_backend(&self) -> Option<Box<dyn ClockBackend>> {
        Clock::detect().map(|clock| Box::new(clock) as Box<dyn ClockBackend>)
    }
}

struct Ring {
    file: File,
}

fn is_logo_ring(geometry: &str, depth: &str) -> bool {
    geometry.trim() == format!("{LED_COUNT},1") && depth.trim() == "24"
}

impl Ring {
    fn open() -> Result<Self> {
        let sysfs = Path::new(FRAMEBUFFER_SYSFS);
        let geometry = fs::read_to_string(sysfs.join("virtual_size"))
            .with_context(|| format!("read {FRAMEBUFFER_SYSFS}/virtual_size"))?;
        let depth = fs::read_to_string(sysfs.join("bits_per_pixel"))
            .with_context(|| format!("read {FRAMEBUFFER_SYSFS}/bits_per_pixel"))?;
        if !is_logo_ring(&geometry, &depth) {
            bail!(
                "{FRAMEBUFFER} is not the Yandex Mini 3 logo ring ({} at {} bits)",
                geometry.trim(),
                depth.trim()
            )
        }
        let file = OpenOptions::new()
            .write(true)
            .open(FRAMEBUFFER)
            .with_context(|| format!("open Yandex Mini 3 logo ring {FRAMEBUFFER}"))?;
        Ok(Self { file })
    }
}

impl LedBackend for Ring {
    fn show(&self, frame: &[u8]) -> Result<()> {
        if frame.len() != CHANNEL_COUNT {
            bail!("unexpected Yandex Mini 3 LED frame length");
        }
        self.file
            .write_all_at(frame, 0)
            .context("write Yandex Mini 3 logo ring")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_describes_mini3_hardware() {
        let profile = YandexMini3.profile();
        assert_eq!(profile.id, "yandexmini3");
        assert_eq!(profile.led_count, LED_COUNT);
        assert!(matches!(profile.color_order, ColorOrder::Bgr));
        assert!(!profile.sensors_proximity);
        assert!(profile.sensors_ambient_light);
    }

    #[test]
    fn accepts_only_the_six_pixel_logo_framebuffer() {
        assert!(is_logo_ring("6,1\n", "24\n"));
        assert!(!is_logo_ring("1280,720\n", "32\n"));
        assert!(!is_logo_ring("6,1\n", "32\n"));
    }
}
