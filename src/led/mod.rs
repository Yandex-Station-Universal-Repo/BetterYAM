mod http;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Mutex;

pub use http::HttpLedSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LedSettings {
    pub backend: String,
    pub http: Option<HttpLedSettings>,
}

impl Default for LedSettings {
    fn default() -> Self {
        Self {
            backend: "http".into(),
            http: None,
        }
    }
}

impl LedSettings {
    pub fn validate(&self) -> Result<()> {
        match self.backend.as_str() {
            "http" => {
                if let Some(http) = &self.http {
                    http.validate()?;
                }
            }
            backend => {
                bail!("unsupported device backend {backend:?}; use the standard HTTP Device API")
            }
        }
        Ok(())
    }
}

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
    #[serde(default)]
    pub capabilities: std::collections::HashMap<String, bool>,
}

#[async_trait]
pub trait LedBackend: Send + Sync {
    async fn draw(&self, colors: &[String]) -> Result<()>;
    async fn off(&self) -> Result<()>;
    async fn device_info(&self) -> Result<DeviceInfo>;
    async fn request_json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value>;
    async fn request_bytes(&self, path: &str, body: Vec<u8>) -> Result<()>;
}

pub struct Led {
    settings: Arc<Mutex<LedSettings>>,
}

impl Clone for Led {
    fn clone(&self) -> Self {
        Self {
            settings: self.settings.clone(),
        }
    }
}

impl Led {
    pub fn new(settings: LedSettings) -> Self {
        Self {
            settings: Arc::new(Mutex::new(settings)),
        }
    }

    pub async fn replace_settings(&self, settings: LedSettings) {
        *self.settings.lock().await = settings;
    }

    pub async fn device_info(&self) -> Result<DeviceInfo> {
        let settings = self.settings.lock().await.clone();
        create_backend(&settings)?.device_info().await
    }

    pub async fn api_json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let settings = self.settings.lock().await.clone();
        create_backend(&settings)?
            .request_json(method, path, body)
            .await
    }

    pub async fn api_bytes(&self, path: &str, body: Vec<u8>) -> Result<()> {
        let settings = self.settings.lock().await.clone();
        create_backend(&settings)?.request_bytes(path, body).await
    }

    pub async fn draw(&self, colors: Vec<String>) -> Result<()> {
        let settings = self.settings.lock().await.clone();
        settings.validate()?;
        if colors.is_empty() || colors.len() > 128 {
            bail!("LED frame must contain 1..128 colors")
        }
        for color in &colors {
            parse_color(color)?;
        }
        create_backend(&settings)?.draw(&colors).await
    }

    pub async fn off(&self) -> Result<()> {
        let settings = self.settings.lock().await.clone();
        settings.validate()?;
        create_backend(&settings)?.off().await
    }

    pub async fn play_led(&self, path: PathBuf, cancelled: &AtomicBool) -> Result<()> {
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("read pattern {}", path.display()))?;
        let (looped, frames) = parse_pattern(&source)?;
        if frames.is_empty() {
            bail!("pattern has no frames")
        }
        loop {
            for frame in &frames {
                self.draw(frame.colors.clone()).await?;
                let mut left = frame.delay_ms;
                while left > 0 {
                    if cancelled.load(Ordering::Relaxed) {
                        bail!("plugin stopped")
                    }
                    let slice = left.min(20);
                    tokio::time::sleep(Duration::from_millis(slice)).await;
                    left -= slice;
                }
            }
            if !looped {
                break;
            }
        }
        Ok(())
    }
}

fn create_backend(settings: &LedSettings) -> Result<Box<dyn LedBackend>> {
    settings.validate()?;
    match settings.backend.as_str() {
        "http" => Ok(Box::new(http::HttpBackend::new(settings.http.clone())?)),
        backend => bail!("unsupported device backend {backend:?}"),
    }
}

fn parse_color(color: &str) -> Result<(u8, u8, u8)> {
    let hex = color.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid RGB color {color}")
    }
    Ok((
        u8::from_str_radix(&hex[0..2], 16)?,
        u8::from_str_radix(&hex[2..4], 16)?,
        u8::from_str_radix(&hex[4..6], 16)?,
    ))
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
        frames.push(Frame {
            colors: parts[..parts.len() - 1]
                .iter()
                .map(|color| format!("#{color}"))
                .collect(),
            delay_ms,
        });
    }
    Ok((looped, frames))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_color_formats_and_rejects_invalid_rgb() {
        assert_eq!(parse_color("#12aBef").unwrap(), (0x12, 0xab, 0xef));
        assert_eq!(parse_color("12aBef").unwrap(), (0x12, 0xab, 0xef));
        assert!(parse_color("#12345g").is_err());
        assert!(parse_color("#12345").is_err());
    }

    #[test]
    fn selects_standard_http_backend_and_rejects_direct_hardware_backends() {
        let settings = LedSettings::default();
        assert!(create_backend(&settings).is_err()); // URL is intentionally required.
        let settings = LedSettings {
            backend: "sysfs".into(),
            ..LedSettings::default()
        };
        assert!(create_backend(&settings).is_err());
    }

    #[test]
    fn legacy_settings_require_configuration_of_external_device_api() {
        let settings: LedSettings = toml::from_str(
            r#"profile = "custom"
count = 9
"#,
        )
        .unwrap();
        assert_eq!(settings.backend, "http");
        assert!(settings.http.is_none());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn optional_api_configuration_serializes_as_valid_toml() {
        assert!(toml::to_string_pretty(&LedSettings::default()).is_ok());
    }
}
