use anyhow::{bail, Context, Result};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const QUASAR_CONFIG: &str = "/system/vendor/quasar/quasar.cfg";
const MAX_FRAME_SIZE: usize = 10 * 1024 * 1024;
const MAX_TIMEOUT_MS: u64 = 60_000;
const IO_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn validate_service(service: &str) -> Result<()> {
    if service.is_empty()
        || service.len() > 64
        || !service
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("invalid Quasar IPC service name")
    }
    Ok(())
}

pub fn service_allowed(services: &[String], service: &str) -> Result<()> {
    validate_service(service)?;
    if !services.iter().any(|allowed| allowed == service) {
        bail!("Quasar IPC service is not allowed by [ipc].services")
    }
    Ok(())
}

pub fn send(service: &str, payload: &[u8], cancelled: &AtomicBool) -> Result<()> {
    ensure_running(cancelled)?;
    validate_frame_size(payload)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let endpoint = endpoint(service)?;
    let mut stream =
        connect(endpoint, deadline, cancelled)?.context("Quasar IPC service is unavailable")?;
    write_frame(&mut stream, payload, deadline, cancelled)
}

pub fn receive(service: &str, timeout_ms: u64, cancelled: &AtomicBool) -> Result<Option<Vec<u8>>> {
    validate_timeout(timeout_ms)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let endpoint = endpoint(service)?;
    let Some(mut stream) = connect(endpoint, deadline, cancelled)? else {
        return Ok(None);
    };
    read_frame(&mut stream, deadline, cancelled)
}

pub fn request(
    service: &str,
    payload: &[u8],
    timeout_ms: u64,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>> {
    validate_timeout(timeout_ms)?;
    ensure_running(cancelled)?;
    validate_frame_size(payload)?;
    let expected_request_id = request_id(payload)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(1));
    let endpoint = endpoint(service)?;
    let mut stream =
        connect(endpoint, deadline, cancelled)?.context("Quasar IPC service is unavailable")?;
    write_frame(&mut stream, payload, deadline, cancelled)?;
    while let Some(response) = read_frame(&mut stream, deadline, cancelled)? {
        if request_id(&response)? == expected_request_id {
            return Ok(Some(response));
        }
    }
    Ok(None)
}

fn validate_timeout(timeout_ms: u64) -> Result<()> {
    if timeout_ms > MAX_TIMEOUT_MS {
        bail!("IPC timeout is limited to 60 seconds")
    }
    Ok(())
}

fn ensure_running(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        bail!("plugin stopped")
    }
    Ok(())
}

fn endpoint(service: &str) -> Result<SocketAddr> {
    validate_service(service)?;
    let variable = format!("YAM_{}_ADDR", service.to_ascii_uppercase());
    let address = match std::env::var(&variable) {
        Ok(address) => address,
        Err(_) => {
            let config = std::fs::read_to_string(QUASAR_CONFIG)
                .with_context(|| format!("read {QUASAR_CONFIG} or set {variable}"))?;
            let port = quasar_service_port(&config, service)?
                .with_context(|| format!("{service}.port is not configured"))?;
            format!("127.0.0.1:{port}")
        }
    };
    let endpoint = address
        .parse::<SocketAddr>()
        .context("invalid IPC address")?;
    if !endpoint.ip().is_ipv4() || !endpoint.ip().is_loopback() {
        bail!("Quasar IPC endpoint must be an IPv4 loopback address")
    }
    Ok(endpoint)
}

fn quasar_service_port(config: &str, service: &str) -> Result<Option<u16>> {
    let value: serde_json::Value =
        serde_json::from_str(config).context("invalid quasar config JSON")?;
    Ok(value
        .get(service)
        .and_then(|entry| entry.get("port"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|port| u16::try_from(port).ok()))
}

fn protobuf_varint(data: &[u8], offset: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *data.get(*offset).context("truncated protobuf field")?;
        *offset += 1;
        if shift == 63 && byte > 1 {
            bail!("protobuf varint overflow")
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    bail!("invalid protobuf varint")
}

fn request_id(data: &[u8]) -> Result<&[u8]> {
    let mut offset = 0;
    while offset < data.len() {
        let key = protobuf_varint(data, &mut offset)?;
        let number = u32::try_from(key >> 3).context("protobuf field number overflow")?;
        if number == 0 || number > 0x1fff_ffff {
            bail!("invalid protobuf field number")
        }
        let wire = (key & 7) as u8;
        if number == 1 && wire == 2 {
            let size = usize::try_from(protobuf_varint(data, &mut offset)?)
                .context("request_id length overflow")?;
            let end = offset
                .checked_add(size)
                .context("request_id length overflow")?;
            return data.get(offset..end).context("truncated request_id field");
        }
        match wire {
            0 => {
                protobuf_varint(data, &mut offset)?;
            }
            1 => {
                offset = offset.checked_add(8).context("protobuf length overflow")?;
                if offset > data.len() {
                    bail!("truncated protobuf field")
                }
            }
            2 => {
                let length = usize::try_from(protobuf_varint(data, &mut offset)?)
                    .context("protobuf length overflow")?;
                offset = offset
                    .checked_add(length)
                    .context("protobuf length overflow")?;
                if offset > data.len() {
                    bail!("truncated protobuf field")
                }
            }
            5 => {
                offset = offset.checked_add(4).context("protobuf length overflow")?;
                if offset > data.len() {
                    bail!("truncated protobuf field")
                }
            }
            _ => bail!("unsupported protobuf wire type"),
        }
    }
    bail!("IPC request must include field 1 request_id")
}

fn validate_frame_size(payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME_SIZE {
        bail!("IPC frame exceeds 10 MiB")
    }
    Ok(())
}

fn write_frame(
    stream: &mut TcpStream,
    payload: &[u8],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<()> {
    validate_frame_size(payload)?;
    let length = i32::try_from(payload.len()).context("IPC frame is too large")?;
    write_all_until(stream, &length.to_le_bytes(), deadline, cancelled)?;
    write_all_until(stream, payload, deadline, cancelled)?;
    stream.flush()?;
    Ok(())
}

fn write_all_until(
    stream: &mut TcpStream,
    data: &[u8],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut offset = 0;
    while offset < data.len() {
        ensure_running(cancelled)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("IPC write timed out")
        }
        stream.set_write_timeout(Some(remaining.min(IO_POLL_INTERVAL)))?;
        match stream.write(&data[offset..]) {
            Ok(0) => bail!("IPC connection closed while writing"),
            Ok(written) => offset += written,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn read_exact_until(
    stream: &mut TcpStream,
    data: &mut [u8],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<bool> {
    let mut offset = 0;
    while offset < data.len() {
        ensure_running(cancelled)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        stream.set_read_timeout(Some(remaining.min(IO_POLL_INTERVAL)))?;
        match stream.read(&mut data[offset..]) {
            Ok(0) => bail!("Quasar IPC connection closed"),
            Ok(read) => offset += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

fn read_frame(
    stream: &mut TcpStream,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>> {
    ensure_running(cancelled)?;
    if deadline.saturating_duration_since(Instant::now()).is_zero() {
        return Ok(None);
    }
    let mut header = [0u8; 4];
    if !read_exact_until(stream, &mut header, deadline, cancelled)? {
        return Ok(None);
    }
    let length = i32::from_le_bytes(header);
    if length < 0 {
        bail!("IPC frame has a negative length")
    }
    let length = usize::try_from(length)?;
    if length > MAX_FRAME_SIZE {
        bail!("IPC frame exceeds 10 MiB")
    }
    let mut payload = vec![0; length];
    if !read_exact_until(stream, &mut payload, deadline, cancelled)? {
        return Ok(None);
    }
    validate_frame_size(&payload)?;
    Ok(Some(payload))
}

fn connect(
    endpoint: SocketAddr,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<Option<TcpStream>> {
    loop {
        ensure_running(cancelled)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match TcpStream::connect_timeout(&endpoint, remaining.min(IO_POLL_INTERVAL)) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                return Ok(Some(stream));
            }
            Err(_) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(None);
                }
                std::thread::sleep(remaining.min(IO_POLL_INTERVAL));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_service_names_are_restricted() {
        assert!(validate_service("mediad").is_ok());
        assert!(validate_service("service_2").is_ok());
        assert!(validate_service("").is_err());
        assert!(validate_service("MediaD").is_err());
        assert!(validate_service("media-daemon").is_err());
        assert!(validate_service("media.daemon").is_err());
        assert!(validate_service(&"a".repeat(65)).is_err());
    }

    #[test]
    fn ipc_access_is_limited_to_declared_services() {
        let allowed = vec!["mediad".to_string()];
        assert!(service_allowed(&allowed, "mediad").is_ok());
        assert!(service_allowed(&allowed, "aliced").is_err());
        assert!(service_allowed(&allowed, "127.0.0.1").is_err());
    }

    #[test]
    fn parses_quasar_service_port_without_exposing_other_config() {
        let config = r#"{"mediad":{"port":1234},"private":{"token":"secret"}}"#;
        assert_eq!(quasar_service_port(config, "mediad").unwrap(), Some(1234));
        assert_eq!(quasar_service_port(config, "missing").unwrap(), None);
        assert_eq!(
            quasar_service_port(r#"{"mediad":{"port":70000}}"#, "mediad").unwrap(),
            None
        );
        assert!(quasar_service_port("not json", "mediad").is_err());
    }

    #[test]
    fn extracts_request_id_and_rejects_malformed_protobuf() {
        let message = [0x0a, 0x02, 0x08, 0x01, 0x10, 0x2a];
        assert_eq!(request_id(&message).unwrap(), [0x08, 0x01]);
        assert!(request_id(&[0]).is_err());
        assert!(request_id(&[0x0a, 0x02, 0x08]).is_err());
    }

    #[test]
    fn rejects_invalid_timeout_and_oversized_messages() {
        assert!(validate_timeout(MAX_TIMEOUT_MS).is_ok());
        assert!(validate_timeout(MAX_TIMEOUT_MS + 1).is_err());
        assert!(validate_frame_size(&vec![0; MAX_FRAME_SIZE + 1]).is_err());
    }
}
