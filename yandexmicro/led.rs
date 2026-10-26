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

pub const LED_COUNT: usize = 3;
pub const CHANNEL_COUNT: usize = LED_COUNT * 3;

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
        let input_devices = crate::input::device_names().await?;
        let input_available = !input_devices.is_empty();
        let mut features = vec![
            "runtime.lua.v1".into(),
            "plugin.yap.v1".into(),
            "led.ring.rgb".into(),
            format!("led.ring.{LED_COUNT}"),
            "led.pattern.v1".into(),
        ];
        if input_available {
            features.extend([
                "input".into(),
                "input.devices".into(),
                "input.next_event".into(),
            ]);
        }
        let volume_available = crate::volume::is_available().await;
        if volume_available {
            features.extend(["volume".into(), "volume.get".into(), "volume.set".into()]);
        }
        let audio_available = crate::audio::available().await;
        if audio_available {
            features.extend(["audio".into(), "audio.play".into()]);
        }
        let sensors_available = crate::sensors::available().await;
        if sensors_available {
            features.extend([
                "sensors".into(),
                "sensors.list".into(),
                "sensors.read".into(),
            ]);
        }
        let bluetooth_capabilities = crate::bluetooth::availability().await?;
        if bluetooth_capabilities.status {
            features.extend(["bluetooth".into(), "bluetooth.status".into()]);
        }
        if bluetooth_capabilities.power {
            features.push("bluetooth.power".into());
        }
        let wifi_available = crate::wifi::available();
        if wifi_available {
            features.extend([
                "wifi".into(),
                "wifi.status".into(),
                "wifi.scan".into(),
                "wifi.networks".into(),
            ]);
        }
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
            ("volume".into(), volume_available),
            ("volume.get".into(), volume_available),
            ("volume.set".into(), volume_available),
            ("audio".into(), audio_available),
            ("audio.play".into(), audio_available),
            ("bluetooth".into(), bluetooth_capabilities.status),
            ("bluetooth.status".into(), bluetooth_capabilities.status),
            ("bluetooth.power".into(), bluetooth_capabilities.power),
            ("display".into(), false),
            ("sensors".into(), sensors_available),
            ("sensors.list".into(), sensors_available),
            ("sensors.read".into(), sensors_available),
            ("sensors.proximity".into(), false),
            ("sensors.control".into(), false),
            ("input".into(), input_available),
            ("input.devices".into(), input_available),
            ("input.next_event".into(), input_available),
            ("ipc.alarm".into(), false),
            ("ipc.speech".into(), false),
            ("wifi".into(), wifi_available),
            ("wifi.status".into(), wifi_available),
            ("wifi.scan".into(), wifi_available),
            ("wifi.networks".into(), wifi_available),
        ]);
        Ok(DeviceInfo {
            id: "yandexmicro".into(),
            model: "yandexmicro".into(),
            led_count: LED_COUNT,
            features,
            capabilities,
        })
    }

    pub async fn api_json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        if method == Method::GET && path == "sensors" {
            let sensors = tokio::task::spawn_blocking(crate::sensors::list)
                .await
                .context("sensor list task failed")??;
            return Ok(serde_json::to_value(sensors)?);
        }
        if method == Method::GET && path.starts_with("sensors/") {
            let id = path.trim_start_matches("sensors/");
            if id.is_empty() || id.contains('/') {
                bail!("invalid sensor ID")
            }
            let id = id.to_owned();
            let sensor = tokio::task::spawn_blocking(move || crate::sensors::read(&id))
                .await
                .context("sensor read task failed")??;
            return Ok(serde_json::to_value(sensor)?);
        }
        if method == Method::GET && path == "input/devices" {
            return Ok(serde_json::to_value(crate::input::device_names().await?)?);
        }
        if method == Method::POST && path == "input/next-event" {
            return crate::input::next_event(body).await;
        }
        if method == Method::GET && path == "volume" {
            return Ok(serde_json::json!(crate::volume::get_percent().await?));
        }
        if method == Method::PUT && path == "volume" {
            return crate::volume::set_percent(body).await;
        }
        if path.starts_with("bluetooth/") {
            return crate::bluetooth::api_json(method, path, body).await;
        }
        bail!(
            "Yandex Lite / Micro does not provide this operation yet: {} /{}",
            method,
            path
        )
    }

    pub async fn api_bytes(
        &self,
        path: &str,
        body: Vec<u8>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        if path == "audio/play" {
            return crate::audio::play(body, cancelled).await;
        }
        bail!("Yandex Lite / Micro does not provide this operation yet: POST /{path}")
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

pub fn parse_color(color: &str) -> Result<(u8, u8, u8)> {
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

pub fn frame_channels(colors: &[String]) -> Result<[u8; CHANNEL_COUNT]> {
    if colors.len() != LED_COUNT {
        bail!("Yandex Lite / Micro LED frame must contain exactly {LED_COUNT} colors")
    }
    let mut frame = [0; CHANNEL_COUNT];
    for (index, color) in colors.iter().enumerate() {
        let (red, green, blue) = parse_color(color)?;
        let offset = index * 3;
        frame[offset] = red;
        frame[offset + 1] = green;
        frame[offset + 2] = blue;
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
struct Driver;

#[cfg(target_os = "linux")]
impl Driver {
    const LED_PATHS: [&'static str; CHANNEL_COUNT] = [
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

    fn new() -> Result<Self> {
        Ok(Self)
    }

    fn show(&self, frame: &[u8; CHANNEL_COUNT]) -> Result<()> {
        for (path, &value) in Self::LED_PATHS.iter().zip(frame.iter()) {
            if std::path::Path::new(path).exists() {
                let _ = std::fs::write(path, value.to_string());
            }
        }
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
struct Driver;

#[cfg(not(target_os = "linux"))]
impl Driver {
    fn new() -> Result<Self> {
        bail!("Yandex Lite / Micro hardware backend is supported only on Linux")
    }

    fn show(&self, _frame: &[u8; CHANNEL_COUNT]) -> Result<()> {
        bail!("Yandex Lite / Micro hardware backend is supported only on Linux")
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
    fn maps_rgb_colors_to_micro_channels_and_checks_count() {
        let frame = frame_channels(&[
            "#123456".into(),
            "#ffffff".into(),
            "#000000".into(),
        ])
        .unwrap();
        assert_eq!(&frame[..3], &[0x12, 0x34, 0x56]);
        assert_eq!(&frame[3..6], &[0xff, 0xff, 0xff]);
        assert_eq!(&frame[6..9], &[0x00, 0x00, 0x00]);
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
