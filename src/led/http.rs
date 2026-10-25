use super::{parse_color, DeviceInfo, LedBackend};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpLedSettings {
    pub base_url: String,
}

impl HttpLedSettings {
    pub fn validate(&self) -> Result<()> {
        let base = Url::parse(&self.base_url).context("invalid device API base_url")?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            bail!("device API base_url must be an absolute HTTP(S) URL")
        }
        if !base.username().is_empty() || base.password().is_some() {
            bail!("device API base_url cannot contain credentials")
        }
        if base.query().is_some() || base.fragment().is_some() {
            bail!("device API base_url cannot contain a query or fragment")
        }
        Ok(())
    }
}

pub struct HttpBackend {
    settings: HttpLedSettings,
    client: Client,
}

impl HttpBackend {
    pub fn new(settings: Option<HttpLedSettings>) -> Result<Self> {
        let settings = settings.context("device API is not configured: set [led.http].base_url")?;
        settings.validate()?;
        Ok(Self {
            settings,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        Url::parse(&self.settings.base_url)?
            .join(path)
            .context("invalid device API endpoint")
    }

    async fn send_json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let timeout = body
            .as_ref()
            .and_then(|value| value.get("timeout_ms"))
            .and_then(Value::as_u64)
            .map(|milliseconds| Duration::from_millis(milliseconds.saturating_add(1_000)))
            .unwrap_or_else(|| Duration::from_secs(10));
        let mut request = self.client.request(method, self.endpoint(path)?);
        request = request.timeout(timeout);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let response_body = response.text().await?;
        if !status.is_success() {
            bail!("device API returned {status}: {response_body}")
        }
        if response_body.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_str(&response_body).context("device API returned invalid JSON")
        }
    }
}

#[async_trait]
impl LedBackend for HttpBackend {
    async fn draw(&self, colors: &[String]) -> Result<()> {
        for color in colors {
            parse_color(color)?;
        }
        self.send_json(
            Method::POST,
            "led",
            Some(serde_json::json!({"colors": colors})),
        )
        .await?;
        Ok(())
    }

    async fn off(&self) -> Result<()> {
        self.send_json(Method::POST, "led/off", None).await?;
        Ok(())
    }

    async fn device_info(&self) -> Result<DeviceInfo> {
        let value = self.send_json(Method::GET, "device", None).await?;
        serde_json::from_value(value).context("device API returned invalid device profile")
    }

    async fn request_json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        self.send_json(method, path, body).await
    }

    async fn request_bytes(&self, path: &str, body: Vec<u8>) -> Result<()> {
        let response = self
            .client
            .post(self.endpoint(path)?)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send()
            .await?;
        let status = response.status();
        let response_body = response.text().await?;
        if !status.is_success() {
            bail!("device API returned {status}: {response_body}")
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[tokio::test]
    async fn sends_standard_led_request_and_reads_device_profile() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /led HTTP/1.1"));
            assert!(request.contains(r##""colors":["#112233"]"##));
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("GET /device HTTP/1.1"));
            let body = r#"{"id":"device-a","model":"Device A","architecture":"aarch64","led_count":1,"features":["led.ring.rgb","led.ring.1"]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let backend = HttpBackend::new(Some(HttpLedSettings {
            base_url: format!("http://{address}/"),
        }))
        .unwrap();
        backend.draw(&["#112233".into()]).await.unwrap();
        let info = backend.device_info().await.unwrap();
        assert_eq!(info.id, "device-a");
        assert_eq!(info.led_count, 1);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn surfaces_device_api_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 11\r\n\r\nunavailable",
                )
                .unwrap();
        });
        let backend = HttpBackend::new(Some(HttpLedSettings {
            base_url: format!("http://{address}/"),
        }))
        .unwrap();
        let error = backend.draw(&["#112233".into()]).await.unwrap_err();
        assert!(error.to_string().contains("503"));
        server.join().unwrap();
    }

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let size = stream.read(&mut chunk).unwrap();
            if size == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..size]);
            if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .or_else(|| line.strip_prefix("Content-Length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }
}
