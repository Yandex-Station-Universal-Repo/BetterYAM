use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

const MIXER_CARD: &str = "0";
const MIXER_CONTROL: &str = "Master";
const STATION_AMIXER_PATHS: &[&str] = &[
    "/usr/bin/amixer",
    "/system/workdir/bin/amixer",
];
const STATION_ALSA_LIBS: &str = "/bkupgrade/data/bundle/ap2/libs:/system/workdir/lib";

#[derive(Debug, PartialEq, Eq)]
struct MixerLevel {
    minimum: i32,
    maximum: i32,
    current: i32,
}

fn parse_master_level(output: &str) -> Result<MixerLevel> {
    let limits = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Limits:"))
        .context("amixer output does not contain Master limits")?;
    let numbers: Vec<i32> = limits
        .split('-')
        .filter_map(|part| {
            part.split_whitespace()
                .find_map(|token| token.parse::<i32>().ok())
        })
        .collect();
    if numbers.len() < 2 {
        bail!("invalid Master limits from amixer: {limits}");
    }
    let minimum = numbers[0];
    let maximum = numbers[1];
    if maximum <= minimum {
        bail!("invalid Master range {minimum}..{maximum}")
    }

    let current = output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            ["Front Left:", "Mono:", "Front:"]
                .iter()
                .find_map(|channel| line.strip_prefix(channel))
        })
        .find_map(|value| {
            value
                .split_whitespace()
                .find_map(|token| token.parse::<i32>().ok())
        })
        .context("amixer output does not contain a Master playback value")?;
    if !(minimum..=maximum).contains(&current) {
        bail!("Master level {current} is outside {minimum}..{maximum}")
    }

    Ok(MixerLevel {
        minimum,
        maximum,
        current,
    })
}

fn run_amixer(args: &[&str]) -> Result<String> {
    let mut chosen_path = "amixer";
    for &candidate in STATION_AMIXER_PATHS {
        if Path::new(candidate).is_file() {
            chosen_path = candidate;
            break;
        }
    }

    let mut cmd = Command::new(chosen_path);
    if chosen_path == "/system/workdir/bin/amixer" {
        cmd.env("LD_LIBRARY_PATH", STATION_ALSA_LIBS);
    }
    let output: Output = cmd
        .args(args)
        .output()
        .context("run amixer for ALSA Master volume")?;
    if !output.status.success() {
        bail!(
            "amixer failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_master_level() -> Result<MixerLevel> {
    let output = run_amixer(&["-c", MIXER_CARD, "get", MIXER_CONTROL])?;
    parse_master_level(&output)
}

fn as_percent(level: &MixerLevel) -> u8 {
    let span = (level.maximum - level.minimum) as f64;
    if span <= 0.0 {
        return 0;
    }
    let fraction = ((level.current - level.minimum) as f64 / span).clamp(0.0, 1.0);
    (fraction * 100.0).round() as u8
}

fn set_master_percent(percent: u8) -> Result<()> {
    let target = format!("{percent}%");
    run_amixer(&["-c", MIXER_CARD, "set", MIXER_CONTROL, &target])?;
    Ok(())
}

pub async fn is_available() -> bool {
    match tokio::task::spawn_blocking(read_master_level).await {
        Ok(Ok(_)) => true,
        Ok(Err(error)) => {
            tracing::warn!(%error, "ALSA Master volume is unavailable");
            false
        }
        Err(error) => {
            tracing::warn!(%error, "ALSA volume detection task failed");
            false
        }
    }
}

pub async fn get_percent() -> Result<u8> {
    tokio::task::spawn_blocking(|| read_master_level().map(|level| as_percent(&level)))
        .await
        .context("ALSA volume read task failed")?
}

pub async fn set_percent(body: Option<Value>) -> Result<Value> {
    let percent = body
        .as_ref()
        .and_then(|value| value.get("percent"))
        .and_then(Value::as_u64)
        .context("volume request requires integer percent in 0..100")?;
    let percent = u8::try_from(percent).context("volume must be 0..100")?;
    tokio::task::spawn_blocking(move || set_master_percent(percent))
        .await
        .context("ALSA volume write task failed")??;
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_alsa_master_control_output() {
        let level = parse_master_level(
            "Simple mixer control 'Master',0\n  Capabilities: volume\n  Limits: 0 - 32\n  Front Left: 16 [50%] [-30.00dB]\n  Front Right: 16 [50%] [-30.00dB]\n",
        )
        .unwrap();
        assert_eq!(
            level,
            MixerLevel {
                minimum: 0,
                maximum: 32,
                current: 16
            }
        );
        assert_eq!(as_percent(&level), 50);
    }

    #[test]
    fn parses_yandex_station_lite_master_output() {
        let level = parse_master_level(
            "Simple mixer control 'Master',0\n  Capabilities: pvolume\n  Playback channels: Front Left - Front Right\n  Limits: Playback 0 - 255\n  Mono:\n  Front Left: Playback 102 [40%] [-30.60dB]\n  Front Right: Playback 102 [40%] [-30.60dB]\n",
        )
        .unwrap();
        assert_eq!(
            level,
            MixerLevel {
                minimum: 0,
                maximum: 255,
                current: 102
            }
        );
        assert_eq!(as_percent(&level), 40);
    }

    #[test]
    fn rejects_unexpected_alsa_control_ranges() {
        assert!(parse_master_level(
            "Simple mixer control 'Master',0\n  Limits: 0 - 0\n  Front Left: 0\n"
        )
        .is_err());
    }
}
