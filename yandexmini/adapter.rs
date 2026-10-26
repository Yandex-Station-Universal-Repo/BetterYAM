use anyhow::{Context, Result};
use betteryam_runtime::device::{AdapterProfile, ColorOrder, DeviceAdapter, LedBackend};
use std::{fs::File, os::fd::AsRawFd};

const LED_COUNT: usize = 6;
const CHANNEL_COUNT: usize = LED_COUNT * 3;

pub struct YandexMini;

impl DeviceAdapter for YandexMini {
    fn profile(&self) -> AdapterProfile {
        AdapterProfile {
            id: "yandexmini".into(),
            model: String::new(),
            led_count: LED_COUNT,
            color_order: ColorOrder::Bgr,
            sensors_proximity: true,
            sensors_ambient_light: false,
        }
    }

    fn create_led_backend(&self) -> Result<Box<dyn LedBackend>> {
        Ok(Box::new(Driver::new()?))
    }
}

struct Driver {
    file: File,
}

#[repr(C)]
struct I2cMessage {
    address: u16,
    flags: u16,
    length: u16,
    buffer: *mut u8,
}

#[repr(C)]
struct I2cRequest {
    messages: *mut I2cMessage,
    count: u32,
}

impl Driver {
    const DEVICE: &'static str = "/dev/i2c-0";
    const ADDRESS: u16 = 0x3c;
    const REG_ENABLE: u8 = 0x00;
    const REG_PWM: u8 = 0x01;
    const REG_UPDATE: u8 = 0x25;
    const REG_MODE: u8 = 0x26;
    const REG_FREQ: u8 = 0x4b;
    #[cfg(target_os = "linux")]
    const I2C_RDWR: libc::c_int = 0x0707;
    #[cfg(not(target_os = "linux"))]
    const I2C_RDWR: libc::c_ulong = 0x0707;

    fn new() -> Result<Self> {
        let file = File::options()
            .read(true)
            .write(true)
            .open(Self::DEVICE)
            .with_context(|| format!("open Yandex Mini I2C device {}", Self::DEVICE))?;
        let driver = Self { file };
        driver.write_register(Self::REG_ENABLE, &[0x01])?;
        driver.write_register(Self::REG_MODE, &[0x03; CHANNEL_COUNT])?;
        driver.write_register(Self::REG_FREQ, &[0x01])?;
        driver.show(&[0; CHANNEL_COUNT])?;
        Ok(driver)
    }

    fn write_register(&self, register: u8, data: &[u8]) -> Result<()> {
        let mut bytes = Vec::with_capacity(data.len() + 1);
        bytes.push(register);
        bytes.extend_from_slice(data);
        let mut message = I2cMessage {
            address: Self::ADDRESS,
            flags: 0,
            length: u16::try_from(bytes.len()).context("I2C payload is too long")?,
            buffer: bytes.as_mut_ptr(),
        };
        let mut request = I2cRequest {
            messages: &mut message,
            count: 1,
        };
        let result = unsafe { libc::ioctl(self.file.as_raw_fd(), Self::I2C_RDWR, &mut request) };
        if result < 0 {
            return Err(std::io::Error::last_os_error()).context("Yandex Mini I2C transfer");
        }
        Ok(())
    }

    fn show(&self, frame: &[u8; CHANNEL_COUNT]) -> Result<()> {
        self.write_register(Self::REG_PWM, frame)?;
        self.write_register(Self::REG_UPDATE, &[0xaa])
    }
}

impl LedBackend for Driver {
    fn show(&self, frame: &[u8]) -> Result<()> {
        let frame: &[u8; CHANNEL_COUNT] = frame
            .try_into()
            .map_err(|_| anyhow::anyhow!("unexpected Yandex Mini LED frame length"))?;
        self.show(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_describes_mini_led_hardware() {
        let profile = YandexMini.profile();
        assert_eq!(profile.led_count, LED_COUNT);
        assert!(matches!(profile.color_order, ColorOrder::Bgr));
        assert!(profile.sensors_proximity);
    }
}
