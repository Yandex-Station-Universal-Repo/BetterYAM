use anyhow::{bail, Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const LED_COUNT: usize = 6;
const CHANNEL_COUNT: usize = LED_COUNT * 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LedSettings {}

#[derive(Debug, Clone, Deserialize)]
pub struct Frame {
    pub colors: Vec<String>,
    #[serde(default = "frame_delay")]
    pub delay_ms: u64,
}

fn frame_delay() -> u64 {
    100
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceInfo {
    pub id: String,
    pub model: String,
    pub architecture: String,
    pub led_count: usize,
    pub features: Vec<String>,
    pub capabilities: HashMap<String, bool>,
}

pub struct Led {
    driver: Arc<Mutex<Driver>>,
}

impl Led {
    pub fn new() -> Result<Self> {
        Ok(Self {
            driver: Arc::new(Mutex::new(Driver::new()?)),
        })
    }

    pub async fn draw(&self, colors: Vec<String>) -> Result<()> {
        self.write_frame(frame_channels(&colors)?).await
    }

    pub async fn off(&self) -> Result<()> {
        self.write_frame([0; CHANNEL_COUNT]).await
    }

    async fn write_frame(&self, frame: [u8; CHANNEL_COUNT]) -> Result<()> {
        let driver = self.driver.clone();
        tokio::task::spawn_blocking(move || {
            driver
                .lock()
                .map_err(|_| anyhow::anyhow!("LED driver lock was poisoned"))?
                .show(&frame)
        })
        .await
        .context("LED write task failed")?
    }

    pub async fn device_info(&self) -> Result<DeviceInfo> {
        let features = vec![
            "runtime.lua.v1".into(),
            "plugin.yap.v1".into(),
            "arch.aarch64".into(),
            "led.ring.rgb".into(),
            format!("led.ring.{LED_COUNT}"),
            "led.pattern.v1".into(),
        ];
        let capabilities = HashMap::from([
            ("led".into(), true),
            ("led.off".into(), true),
            ("led.ring".into(), true),
            ("led.ring.rgb".into(), true),
            (format!("led.ring.{LED_COUNT}"), true),
            ("led.pattern".into(), true),
            ("clock".into(), false),
            ("clock.segments".into(), false),
            ("clock.time".into(), false),
            ("volume".into(), false),
            ("audio".into(), false),
            ("bluetooth".into(), false),
            ("display".into(), false),
            ("sensors".into(), false),
            ("sensors.control".into(), false),
            ("input".into(), false),
            ("ipc.alarm".into(), false),
            ("ipc.speech".into(), false),
        ]);
        Ok(DeviceInfo {
            id: "yandexmini".into(),
            model: "yandexmini".into(),
            architecture: std::env::consts::ARCH.into(),
            led_count: LED_COUNT,
            features,
            capabilities,
        })
    }

    pub async fn api_json(
        &self,
        method: Method,
        path: &str,
        _body: Option<Value>,
    ) -> Result<Value> {
        bail!(
            "Yandex Mini does not provide this operation yet: {} /{}",
            method,
            path
        )
    }

    pub async fn api_bytes(&self, path: &str, _body: Vec<u8>) -> Result<()> {
        bail!("Yandex Mini does not provide this operation yet: POST /{path}")
    }

    pub async fn play_led(&self, path: PathBuf, cancelled: &AtomicBool) -> Result<()> {
        let pattern = std::fs::read_to_string(&path)
            .with_context(|| format!("read pattern {}", path.display()))?;
        let (looped, frames) = parse_pattern(&pattern)?;
        if frames.is_empty() {
            bail!("pattern has no frames")
        }
        loop {
            for frame in &frames {
                if cancelled.load(Ordering::Relaxed) {
                    bail!("plugin stopped")
                }
                self.draw(frame.colors.clone()).await?;
                let mut remaining = frame.delay_ms;
                while remaining > 0 {
                    if cancelled.load(Ordering::Relaxed) {
                        bail!("plugin stopped")
                    }
                    let interval = remaining.min(20);
                    tokio::time::sleep(Duration::from_millis(interval)).await;
                    remaining -= interval;
                }
            }
            if !looped {
                break;
            }
        }
        Ok(())
    }
}

fn parse_color(color: &str) -> Result<(u8, u8, u8)> {
    let value = color.trim().trim_start_matches('#');
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid RGB color {color}")
    }
    Ok((
        u8::from_str_radix(&value[0..2], 16)?,
        u8::from_str_radix(&value[2..4], 16)?,
        u8::from_str_radix(&value[4..6], 16)?,
    ))
}

fn frame_channels(colors: &[String]) -> Result<[u8; CHANNEL_COUNT]> {
    if colors.len() != LED_COUNT {
        bail!("Yandex Mini LED frame must contain exactly {LED_COUNT} colors")
    }
    let mut frame = [0; CHANNEL_COUNT];
    for (index, color) in colors.iter().enumerate() {
        let (red, green, blue) = parse_color(color)?;
        let offset = index * 3;
        frame[offset] = blue;
        frame[offset + 1] = green;
        frame[offset + 2] = red;
    }
    Ok(frame)
}

fn parse_pattern(source: &str) -> Result<(bool, Vec<Frame>)> {
    let mut looped = false;
    let mut frames = Vec::new();
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        if line.eq_ignore_ascii_case("loop") {
            looped = true;
            continue;
        }
        if line.eq_ignore_ascii_case("gammacorrection") {
            continue;
        }
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let delay_ms = parts.last().context("missing frame delay")?.parse()?;
        let colors = parts[..parts.len() - 1]
            .iter()
            .map(|color| format!("#{color}"))
            .collect::<Vec<_>>();
        frame_channels(&colors)?;
        frames.push(Frame { colors, delay_ms });
    }
    Ok((looped, frames))
}

#[cfg(target_os = "linux")]
struct Driver {
    file: std::fs::File,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct I2cMessage {
    address: u16,
    flags: u16,
    length: u16,
    buffer: *mut u8,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct I2cRequest {
    messages: *mut I2cMessage,
    count: u32,
}

#[cfg(target_os = "linux")]
impl Driver {
    const DEVICE: &'static str = "/dev/i2c-0";
    const ADDRESS: u16 = 0x3c;
    const REG_ENABLE: u8 = 0x00;
    const REG_PWM: u8 = 0x01;
    const REG_UPDATE: u8 = 0x25;
    const REG_MODE: u8 = 0x26;
    const REG_FREQ: u8 = 0x4b;
    const I2C_RDWR: libc::c_int = 0x0707;

    fn new() -> Result<Self> {
        let file = std::fs::OpenOptions::new()
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
        use std::os::fd::AsRawFd;
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
        // I2C_RDWR is the Linux combined-transfer ioctl; the Mini's address
        // and controller registers are intentionally confined to this device backend.
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

#[cfg(not(target_os = "linux"))]
struct Driver;

#[cfg(not(target_os = "linux"))]
impl Driver {
    fn new() -> Result<Self> {
        bail!("Yandex Mini hardware backend is supported only on Linux")
    }

    fn show(&self, _frame: &[u8; CHANNEL_COUNT]) -> Result<()> {
        bail!("Yandex Mini hardware backend is supported only on Linux")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_color("#12aBef").unwrap(), (0x12, 0xab, 0xef));
        assert!(parse_color("bad").is_err());
    }

    #[test]
    fn maps_rgb_colors_to_mini_bgr_channels_and_checks_count() {
        let frame = frame_channels(&[
            "#123456".into(),
            "#ffffff".into(),
            "#000000".into(),
            "#000000".into(),
            "#000000".into(),
            "#000000".into(),
        ])
        .unwrap();
        assert_eq!(&frame[..3], &[0x56, 0x34, 0x12]);
        assert!(frame_channels(&["#123456".into()]).is_err());
    }

    #[test]
    fn legacy_led_settings_are_ignored() {
        let settings: LedSettings = toml::from_str(
            r#"backend = "http"
[http]
base_url = "http://example.test"
"#,
        )
        .unwrap();
        assert!(toml::to_string(&settings).is_ok());
    }
}
