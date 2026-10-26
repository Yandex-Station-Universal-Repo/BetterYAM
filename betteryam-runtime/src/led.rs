use crate::device::{
    AdapterProfile, ClockBackend, ClockCommand, ColorOrder, DeviceAdapter, LedBackend,
};
use anyhow::{bail, Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

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

#[derive(Debug, Deserialize)]
struct ClockRequest {
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    brightness: Option<u8>,
    #[serde(default)]
    segments: Option<HashMap<String, u8>>,
    #[serde(default)]
    off: bool,
}

const DEFAULT_CLOCK_BRIGHTNESS: u8 = 170;

fn clock_command(body: Option<Value>) -> Result<ClockCommand> {
    let request: ClockRequest =
        serde_json::from_value(body.context("clock request body is required")?)
            .context("invalid clock request")?;
    if request.off {
        Ok(ClockCommand::Off)
    } else if let Some(segments) = request.segments {
        Ok(ClockCommand::Segments(segments))
    } else if let Some(text) = request.time {
        Ok(ClockCommand::Time {
            text,
            brightness: request.brightness.unwrap_or(DEFAULT_CLOCK_BRIGHTNESS),
        })
    } else {
        bail!("pass time, segments or off=true")
    }
}

pub struct Led {
    backend: Arc<dyn LedBackend>,
    clock: Option<Arc<dyn ClockBackend>>,
    adapter: AdapterProfile,
}

impl Led {
    pub fn new(adapter: impl DeviceAdapter) -> Result<Self> {
        let info = adapter.profile();
        if info.led_count == 0 {
            bail!("device adapter must provide at least one LED");
        }
        Ok(Self {
            backend: Arc::from(adapter.create_led_backend()?),
            clock: adapter.create_clock_backend().map(Arc::from),
            adapter: info,
        })
    }

    pub fn clock_segments(&self) -> Vec<String> {
        self.clock
            .as_ref()
            .map(|clock| clock.segments())
            .unwrap_or_default()
    }

    pub async fn draw(&self, colors: Vec<String>) -> Result<()> {
        self.write_frame(frame_channels(
            &colors,
            self.adapter.led_count,
            self.adapter.color_order,
        )?)
        .await
    }

    pub async fn off(&self) -> Result<()> {
        self.write_frame(vec![0; self.adapter.led_count * 3]).await
    }

    async fn write_frame(&self, frame: Vec<u8>) -> Result<()> {
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || backend.show(&frame))
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
            format!("led.ring.{}", self.adapter.led_count),
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
        let sensors_available = crate::sensors::available(self.adapter.sensors_ambient_light).await;
        if sensors_available {
            features.extend([
                "sensors".into(),
                "sensors.list".into(),
                "sensors.read".into(),
            ]);
            if self.adapter.sensors_proximity {
                features.push("sensors.proximity".into());
            }
            if self.adapter.sensors_ambient_light {
                features.push("sensors.ambient_light".into());
            }
        }
        let bluetooth_capabilities = crate::bluetooth::availability().await?;
        if bluetooth_capabilities.status {
            features.extend(["bluetooth".into(), "bluetooth.status".into()]);
        }
        if bluetooth_capabilities.power {
            features.push("bluetooth.power".into());
        }
        let clock_available = self.clock.is_some();
        if clock_available {
            features.extend([
                "clock".into(),
                "clock.available".into(),
                "clock.time".into(),
                "clock.segments".into(),
                "clock.off".into(),
                "display.clock.segment.v1".into(),
            ]);
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
        let led_count = self.adapter.led_count;
        let capabilities = HashMap::from([
            ("led".into(), true),
            ("led.off".into(), true),
            ("led.ring".into(), true),
            ("led.ring.rgb".into(), true),
            (format!("led.ring.{led_count}"), true),
            ("led.pattern".into(), true),
            ("clock".into(), clock_available),
            ("clock.available".into(), clock_available),
            ("clock.time".into(), clock_available),
            ("clock.segments".into(), clock_available),
            ("clock.off".into(), clock_available),
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
            (
                "sensors.proximity".into(),
                sensors_available && self.adapter.sensors_proximity,
            ),
            (
                "sensors.ambient_light".into(),
                sensors_available && self.adapter.sensors_ambient_light,
            ),
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
            id: self.adapter.id.clone(),
            model: self.adapter.model.clone(),
            led_count,
            features,
            capabilities,
        })
    }

    pub async fn api_json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        if method == Method::GET && path == "sensors" {
            let ambient_light = self.adapter.sensors_ambient_light;
            let sensors = tokio::task::spawn_blocking(move || crate::sensors::list(ambient_light))
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
            let ambient_light = self.adapter.sensors_ambient_light;
            let sensor =
                tokio::task::spawn_blocking(move || crate::sensors::read(&id, ambient_light))
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
        if method == Method::PUT && path == "clock" {
            let clock = self
                .clock
                .clone()
                .context("device does not provide clock segments")?;
            let command = clock_command(body)?;
            tokio::task::spawn_blocking(move || clock.apply(command))
                .await
                .context("clock write task failed")??;
            return Ok(Value::Null);
        }
        bail!(
            "device does not provide this operation yet: {} /{}",
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
        bail!("device does not provide this operation yet: POST /{path}")
    }

    pub async fn play_led(&self, path: PathBuf, cancelled: &AtomicBool) -> Result<()> {
        let pattern = std::fs::read_to_string(&path)
            .with_context(|| format!("read pattern {}", path.display()))?;
        let (looped, frames) = parse_pattern(&pattern, self.adapter.led_count)?;
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

fn frame_channels(colors: &[String], led_count: usize, color_order: ColorOrder) -> Result<Vec<u8>> {
    if colors.len() != led_count {
        bail!("LED frame must contain exactly {led_count} colors")
    }
    let mut frame = Vec::with_capacity(led_count * 3);
    for color in colors {
        let (red, green, blue) = parse_color(color)?;
        match color_order {
            ColorOrder::Rgb => frame.extend([red, green, blue]),
            ColorOrder::Bgr => frame.extend([blue, green, red]),
        }
    }
    Ok(frame)
}

fn parse_pattern(source: &str, led_count: usize) -> Result<(bool, Vec<Frame>)> {
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
        frame_channels(&colors, led_count, ColorOrder::Rgb)?;
        frames.push(Frame { colors, delay_ms });
    }
    Ok((looped, frames))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rgb_frames_and_maps_device_color_order() {
        let colors = vec!["#123456".to_string()];
        assert_eq!(
            frame_channels(&colors, 1, ColorOrder::Rgb).unwrap(),
            [0x12, 0x34, 0x56]
        );
        assert_eq!(
            frame_channels(&colors, 1, ColorOrder::Bgr).unwrap(),
            [0x56, 0x34, 0x12]
        );
        assert!(frame_channels(&colors, 2, ColorOrder::Rgb).is_err());
        assert!(frame_channels(&["bad".into()], 1, ColorOrder::Rgb).is_err());
    }

    #[test]
    fn clock_requests_become_backend_commands() {
        assert_eq!(
            clock_command(Some(serde_json::json!({"time": "12:34"}))).unwrap(),
            ClockCommand::Time {
                text: "12:34".into(),
                brightness: DEFAULT_CLOCK_BRIGHTNESS
            }
        );
        assert_eq!(
            clock_command(Some(
                serde_json::json!({"segments": {"A0": 9}, "time": "1"})
            ))
            .unwrap(),
            ClockCommand::Segments(HashMap::from([("A0".into(), 9)]))
        );
        assert_eq!(
            clock_command(Some(serde_json::json!({"off": true, "time": "1"}))).unwrap(),
            ClockCommand::Off
        );
        assert!(clock_command(Some(serde_json::json!({"brightness": 3}))).is_err());
        assert!(clock_command(None).is_err());
    }

    #[test]
    fn parses_patterns_using_the_selected_device_led_count() {
        let (looped, frames) = parse_pattern("123456 100\n", 1).unwrap();
        assert!(!looped);
        assert_eq!(frames.len(), 1);
        assert!(parse_pattern("123456 100\n", 2).is_err());
    }
}
