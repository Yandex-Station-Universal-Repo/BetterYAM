use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

const MIXER_CARD: &str = "0";
const MIXER_CONTROL: &str = "Master";
const STATION_AMIXER: &str = "/system/workdir/bin/amixer";
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
    let (minimum, maximum) = limits
        .trim()
        .split_once('-')
        .context("invalid Master limits from amixer")?;
    let minimum: i32 = minimum.trim().parse().context("invalid Master minimum")?;
    let maximum: i32 = maximum.trim().parse().context("invalid Master maximum")?;
    if maximum <= minimum {
        bail!("invalid Master range {minimum}..{maximum}")
    }

    let current = output
        .lines()
        .find_map(|line| {
            let line = line.trim();
            ["Front Left:", "Mono:", "Front:"]
                .iter()
                .find_map(|channel| line.strip_prefix(channel))
        })
        .and_then(|value| value.split_whitespace().next())
        .context("amixer output does not contain a Master playback value")?
        .parse::<i32>()
        .context("invalid Master playback value")?;
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
    let output: Output = if Path::new(STATION_AMIXER).is_file() {
        Command::new(STATION_AMIXER)
            .env("LD_LIBRARY_PATH", STATION_ALSA_LIBS)
            .args(args)
            .output()
            .context("run station amixer for ALSA Master volume")?
    } else {
        Command::new("amixer")
            .args(args)
            .output()
            .context("run amixer for ALSA Master volume")?
    };
    if !output.status.success() {
        bail!(
            "amixer failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
    String::from_utf8(output.stdout).context("amixer output is not UTF-8")
}

fn read_master_level() -> Result<MixerLevel> {
    let output = run_amixer(&["-c", MIXER_CARD, "get", MIXER_CONTROL])?;
    parse_master_level(&output)
}

fn as_percent(level: &MixerLevel) -> u8 {
    let span = i64::from(level.maximum) - i64::from(level.minimum);
    let scaled = (i64::from(level.current) - i64::from(level.minimum)) * 100;
    ((scaled + span / 2) / span) as u8
}

fn set_master_percent(percent: u8) -> Result<()> {
    if percent > 100 {
        bail!("volume must be 0..100")
    }
    let level = read_master_level()?;
    let span = i64::from(level.maximum) - i64::from(level.minimum);
    let target = i64::from(level.minimum) + (i64::from(percent) * span + 50) / 100;
    let target = i32::try_from(target).context("calculated ALSA level is out of range")?;
    let target = target.to_string();
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
    fn rejects_unexpected_alsa_control_ranges() {
        assert!(parse_master_level(
            "Simple mixer control 'Master',0\n  Limits: 0 - 0\n  Front Left: 0\n"
        )
        .is_err());
    }
}
