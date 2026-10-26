use anyhow::{bail, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const APLAY_PATHS: &[&str] = &["/system/workdir/bin/aplay", "/usr/bin/aplay"];
const ALSA_LIBS: &str = "/bkupgrade/data/bundle/ap2/libs:/system/workdir/lib";
const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_PLAYBACK_SECONDS: u64 = 600;
const STOP_GRACE: Duration = Duration::from_secs(5);

struct TemporaryAudio(PathBuf);

impl Drop for TemporaryAudio {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path=%self.0.display(), %error, "could not remove temporary audio file");
            }
        }
    }
}

fn find_aplay() -> Option<PathBuf> {
    let from_path = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join("aplay"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    from_path
        .into_iter()
        .chain(APLAY_PATHS.iter().map(PathBuf::from))
        .find(|path| path.is_file())
}

fn command(path: &PathBuf) -> Command {
    let mut command = Command::new(path);
    if path.as_path() == PathBuf::from(APLAY_PATHS[0]) {
        command.env("LD_LIBRARY_PATH", ALSA_LIBS);
    }
    command
}

fn available_blocking() -> Result<bool> {
    let Some(aplay) = find_aplay() else {
        return Ok(false);
    };
    let output = command(&aplay)
        .args(["--list-devices"])
        .output()
        .with_context(|| format!("run {}", aplay.display()))?;
    if !output.status.success() {
        bail!(
            "aplay could not enumerate playback devices: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    Ok(listing.contains("PLAYBACK Hardware Devices") && listing.contains("card "))
}

pub async fn available() -> bool {
    match tokio::task::spawn_blocking(available_blocking).await {
        Ok(Ok(available)) => available,
        Ok(Err(error)) => {
            tracing::warn!(%error, "ALSA audio playback is unavailable");
            false
        }
        Err(error) => {
            tracing::warn!(%error, "ALSA audio detection task failed");
            false
        }
    }
}

fn wav_duration(bytes: &[u8]) -> Result<u64> {
    if bytes.len() > MAX_AUDIO_BYTES {
        bail!("audio file exceeds 64 MiB")
    }
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("audio playback accepts PCM WAV files only")
    }
    let riff_end = u32::from_le_bytes(bytes[4..8].try_into()?) as usize + 8;
    if riff_end > bytes.len() {
        bail!("WAV file is truncated")
    }
    let mut offset = 12usize;
    let mut format = None;
    let mut data_len = None;
    while offset.checked_add(8).is_some_and(|end| end <= riff_end) {
        let chunk_name = &bytes[offset..offset + 4];
        let chunk_len = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into()?) as usize;
        let chunk_start = offset + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_len)
            .context("WAV chunk size overflow")?;
        if chunk_end > riff_end {
            bail!("WAV chunk extends beyond the RIFF container")
        }
        if chunk_name == b"fmt " {
            if chunk_len < 16 {
                bail!("WAV format chunk is too short")
            }
            let encoding = u16::from_le_bytes(bytes[chunk_start..chunk_start + 2].try_into()?);
            let channels = u16::from_le_bytes(bytes[chunk_start + 2..chunk_start + 4].try_into()?);
            let sample_rate =
                u32::from_le_bytes(bytes[chunk_start + 4..chunk_start + 8].try_into()?);
            let bits = u16::from_le_bytes(bytes[chunk_start + 14..chunk_start + 16].try_into()?);
            if ![1, 3].contains(&encoding)
                || !(1..=2).contains(&channels)
                || !(8_000..=192_000).contains(&sample_rate)
                || ![8, 16, 24, 32].contains(&bits)
            {
                bail!("WAV must be mono/stereo PCM or float at a supported sample rate")
            }
            format = Some((u64::from(channels), u64::from(sample_rate), u64::from(bits)));
        } else if chunk_name == b"data" {
            data_len = Some(chunk_len as u64);
        }
        offset = chunk_end + (chunk_len & 1);
    }
    let (channels, sample_rate, bits) = format.context("WAV format chunk is missing")?;
    let data_len = data_len.context("WAV audio data chunk is missing")?;
    let bytes_per_second = channels * sample_rate * bits / 8;
    if bytes_per_second == 0 || data_len == 0 {
        bail!("WAV audio data is empty")
    }
    let duration = data_len.div_ceil(bytes_per_second);
    if duration > MAX_PLAYBACK_SECONDS {
        bail!("audio playback is limited to {MAX_PLAYBACK_SECONDS} seconds")
    }
    Ok(duration)
}

pub fn validate(bytes: &[u8]) -> Result<()> {
    wav_duration(bytes).map(|_| ())
}

fn stop(child: &mut Child) -> Result<()> {
    let pid = libc::pid_t::try_from(child.id()).context("aplay process ID is out of range")?;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .context("wait for stopped audio playback")?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    tracing::warn!("aplay ignored SIGTERM; killing it");
    child.kill().context("kill audio playback")?;
    child.wait().context("wait for killed audio playback")?;
    Ok(())
}

fn play_blocking(bytes: Vec<u8>, cancelled: Arc<AtomicBool>) -> Result<()> {
    wav_duration(&bytes)?;
    if cancelled.load(Ordering::Relaxed) {
        bail!("plugin stopped")
    }
    let aplay = find_aplay().context("aplay is not available on this station")?;
    let path = std::env::temp_dir().join(format!("betteryam-audio-{}.wav", uuid::Uuid::new_v4()));
    let _temporary_audio = TemporaryAudio(path.clone());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("create temporary audio file {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write temporary audio file {}", path.display()))?;
    drop(file);

    let mut child = command(&aplay)
        .arg("--quiet")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .with_context(|| format!("start {}", aplay.display()))?;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            stop(&mut child)?;
            bail!("plugin stopped")
        }
        if let Some(status) = child.try_wait().context("wait for audio playback")? {
            if status.success() {
                return Ok(());
            }
            bail!("aplay failed with status {status}")
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub async fn play(bytes: Vec<u8>, cancelled: Arc<AtomicBool>) -> Result<()> {
    tokio::task::spawn_blocking(move || play_blocking(bytes, cancelled))
        .await
        .context("audio playback task failed")?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(duration_samples: u32) -> Vec<u8> {
        let data_len = duration_samples * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8_000u32.to_le_bytes());
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(44 + data_len as usize, 0);
        bytes
    }

    #[test]
    fn accepts_bounded_pcm_wav() {
        assert_eq!(wav_duration(&wav(8000)).unwrap(), 1);
    }

    #[test]
    fn rejects_non_wav_and_excessive_playback() {
        assert!(wav_duration(b"not wav").is_err());
        assert!(wav_duration(&wav((MAX_PLAYBACK_SECONDS as u32 + 1) * 8_000)).is_err());
    }
}
