use anyhow::{bail, Context, Result};
use betteryam_runtime::device::{ClockBackend, ClockCommand};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const LEDS_ROOT: &str = "/sys/class/leds";
const SEGMENT_PREFIX: &str = "bergamot:white:clock";
const DISPLAY_SETTINGS: &str = "/data/quasar/data/clock_display_settings.dat";
const GEOSYNC_STATE: &str = "/data/quasar/data/geosync.state";
const DIGITS: usize = 4;
const PARTS: [char; 7] = ['A', 'B', 'C', 'D', 'E', 'F', 'G'];
const MARKS: [&str; 4] = ["DEGREE", "DH", "DL", "DM"];

fn segment_names() -> Vec<String> {
    let mut names = Vec::with_capacity(DIGITS * PARTS.len() + MARKS.len());
    for digit in 0..DIGITS {
        for part in PARTS {
            names.push(format!("{part}{digit}"));
        }
    }
    names.extend(MARKS.iter().map(|mark| mark.to_string()));
    names
}

fn glyph(character: char) -> Option<&'static str> {
    Some(match character {
        '0' => "ABCDEF",
        '1' => "BC",
        '2' => "ABDEG",
        '3' => "ABCDG",
        '4' => "BCFG",
        '5' => "ACDFG",
        '6' => "ACDEFG",
        '7' => "ABC",
        '8' => "ABCDEFG",
        '9' => "ABCDFG",
        '-' => "G",
        ' ' => "",
        _ => return None,
    })
}

type Levels = HashMap<String, u8>;

fn blank() -> Levels {
    segment_names().into_iter().map(|name| (name, 0)).collect()
}

fn segments_frame(requested: &HashMap<String, u8>) -> Result<Levels> {
    let mut frame = blank();
    for (name, level) in requested {
        let Some(slot) = frame.get_mut(name) else {
            bail!(
                "unknown Yandex Mini 3 clock segment '{name}'; available: {}",
                segment_names().join(", ")
            )
        };
        *slot = *level;
    }
    Ok(frame)
}

fn parse_time(text: &str) -> Result<(String, bool)> {
    let (characters, colon) = match text.split_once(':') {
        Some((left, right)) => {
            if left.is_empty() || left.chars().count() > 2 || right.chars().count() != 2 {
                bail!("clock time must look like HH:MM")
            }
            (format!("{left:>2}{right}"), true)
        }
        None => {
            if text.chars().count() > DIGITS {
                bail!("clock text is limited to {DIGITS} characters")
            }
            (format!("{text:>4}"), false)
        }
    };
    if let Some(bad) = characters.chars().find(|c| glyph(*c).is_none()) {
        bail!("the clock cannot show '{bad}'; use digits, '-' or spaces")
    }
    Ok((characters, colon))
}

fn text_frame(characters: &str, colon: bool, level: u8) -> Levels {
    let mut frame = blank();
    for (digit, character) in characters.chars().take(DIGITS).enumerate() {
        for part in glyph(character).unwrap_or_default().chars() {
            frame.insert(format!("{part}{digit}"), level);
        }
    }
    if colon {
        frame.insert("DH".into(), level);
        frame.insert("DL".into(), level);
    }
    frame
}

#[derive(Debug, PartialEq)]
struct StockDisplay {
    enabled: bool,
    level: u8,
}

fn parse_display_settings(raw: &str) -> Option<StockDisplay> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let brightness = value.get("brightness")?.as_f64()?;
    let enabled = value
        .get("clockEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    Some(StockDisplay {
        enabled,
        level: (brightness.clamp(0.0, 1.0) * 255.0).round() as u8,
    })
}

fn varint(bytes: &[u8], position: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*position)?;
        *position += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn protobuf_fields(bytes: &[u8]) -> Option<Vec<(u64, Result<u64, &[u8]>)>> {
    let mut fields = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        let key = varint(bytes, &mut position)?;
        match key & 7 {
            0 => fields.push((key >> 3, Ok(varint(bytes, &mut position)?))),
            2 => {
                let length = usize::try_from(varint(bytes, &mut position)?).ok()?;
                let end = position.checked_add(length)?;
                fields.push((key >> 3, Err(bytes.get(position..end)?)));
                position = end;
            }
            _ => return None,
        }
    }
    Some(fields)
}

fn utc_offset_seconds(geosync: &[u8]) -> Option<i64> {
    let zone = protobuf_fields(geosync)?
        .into_iter()
        .find_map(|(field, value)| (field == 2).then_some(value.err()).flatten())?;
    let offset = protobuf_fields(zone)?
        .into_iter()
        .find_map(|(field, value)| (field == 2).then_some(value.ok()).flatten())?;
    let offset = offset as i64;
    (-14 * 3600..=14 * 3600).contains(&offset).then_some(offset)
}

fn local_hhmm(unix_seconds: i64, offset: i64) -> String {
    let minutes = (unix_seconds + offset).div_euclid(60);
    let day_minutes = minutes.rem_euclid(24 * 60);
    format!("{:02}{:02}", day_minutes / 60, day_minutes % 60)
}

fn stock_frame(display: Option<StockDisplay>, offset: Option<i64>, unix_seconds: i64) -> Levels {
    match (display, offset) {
        (Some(display), Some(offset)) if display.enabled => text_frame(
            &local_hhmm(unix_seconds, offset),
            true,
            display.level.max(1),
        ),
        _ => blank(),
    }
}

pub struct Clock {
    segments: Mutex<Vec<(String, File)>>,
}

impl Clock {
    pub fn detect() -> Option<Self> {
        let mut segments = Vec::new();
        for name in segment_names() {
            let path = Path::new(LEDS_ROOT)
                .join(format!("{SEGMENT_PREFIX}{name}"))
                .join("brightness");
            let file = OpenOptions::new().write(true).open(path).ok()?;
            segments.push((name, file));
        }
        Some(Self {
            segments: Mutex::new(segments),
        })
    }

    fn stock(&self) -> Levels {
        let display = fs::read_to_string(DISPLAY_SETTINGS)
            .ok()
            .and_then(|raw| parse_display_settings(&raw));
        let offset = fs::read(GEOSYNC_STATE)
            .ok()
            .and_then(|raw| utc_offset_seconds(&raw));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or_default();
        stock_frame(display, offset, now)
    }

    fn write(&self, frame: &Levels) -> Result<()> {
        let mut segments = self
            .segments
            .lock()
            .map_err(|_| anyhow::anyhow!("clock lock was poisoned"))?;
        for (name, file) in segments.iter_mut() {
            let level = frame.get(name).copied().unwrap_or_default();
            file.write_all(level.to_string().as_bytes())
                .with_context(|| format!("write clock segment {name}"))?;
        }
        Ok(())
    }
}

impl ClockBackend for Clock {
    fn segments(&self) -> Vec<String> {
        segment_names()
    }

    fn apply(&self, command: ClockCommand) -> Result<()> {
        let frame = match command {
            ClockCommand::Off => self.stock(),
            ClockCommand::Segments(segments) => segments_frame(&segments)?,
            ClockCommand::Time { text, brightness } => {
                let (characters, colon) = parse_time(&text)?;
                text_frame(&characters, colon, brightness)
            }
        };
        self.write(&frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_all_sysfs_segments() {
        let names = segment_names();
        assert_eq!(names.len(), 32);
        assert_eq!(names[0], "A0");
        assert_eq!(names[27], "G3");
        assert_eq!(&names[28..], ["DEGREE", "DH", "DL", "DM"]);
    }

    #[test]
    fn segment_frames_replace_the_whole_display() {
        let frame = segments_frame(&HashMap::from([("A0".into(), 200), ("DM".into(), 5)])).unwrap();
        assert_eq!(frame["A0"], 200);
        assert_eq!(frame["DM"], 5);
        assert_eq!(frame["B0"], 0);
        assert_eq!(frame.len(), 32);
        assert!(segments_frame(&HashMap::from([("H0".into(), 1)])).is_err());
    }

    #[test]
    fn parses_clock_times() {
        assert_eq!(parse_time("12:34").unwrap(), ("1234".into(), true));
        assert_eq!(parse_time("7:05").unwrap(), (" 705".into(), true));
        assert_eq!(parse_time("42").unwrap(), ("  42".into(), false));
        assert!(parse_time("123:4").is_err());
        assert!(parse_time("12:345").is_err());
        assert!(parse_time("12345").is_err());
        assert!(parse_time("ab:cd").is_err());
    }

    #[test]
    fn renders_digits_and_colon() {
        let frame = text_frame("1234", true, 90);
        assert_eq!(frame["B0"], 90);
        assert_eq!(frame["C0"], 90);
        assert_eq!(frame["A0"], 0);
        assert_eq!(frame["G1"], 90);
        assert_eq!(frame["F3"], 90);
        assert_eq!(frame["DH"], 90);
        assert_eq!(frame["DL"], 90);
        assert_eq!(frame["DM"], 0);
        assert_eq!(text_frame(" 705", false, 90)["DH"], 0);
    }

    #[test]
    fn reads_the_firmware_display_settings() {
        let display = parse_display_settings(
            r#"{"brightness":0.35686274509803922,"clockEnabled":true,"isClockOn":true}"#,
        )
        .unwrap();
        assert_eq!(
            display,
            StockDisplay {
                enabled: true,
                level: 91
            }
        );
        assert!(
            !parse_display_settings(r#"{"brightness":1,"clockEnabled":false}"#)
                .unwrap()
                .enabled
        );
        assert!(parse_display_settings("not json").is_none());
    }

    #[test]
    fn reads_the_utc_offset_from_geosync_state() {
        let mut zone = vec![0x0a, 4];
        zone.extend_from_slice(b"Zone");
        zone.extend_from_slice(&[0x10, 0xb0, 0x54]);
        let mut state = vec![0x0a, 0x00, 0x12, zone.len() as u8];
        state.extend_from_slice(&zone);
        assert_eq!(utc_offset_seconds(&state), Some(10800));
        assert_eq!(utc_offset_seconds(&[0x0a, 0x00]), None);
        assert_eq!(utc_offset_seconds(&[0xff]), None);
    }

    #[test]
    fn hands_the_display_back_with_the_current_time() {
        let display = Some(StockDisplay {
            enabled: true,
            level: 91,
        });
        let frame = stock_frame(display, Some(10800), 1_761_476_400 + 125);
        assert_eq!(local_hhmm(1_761_476_400 + 125, 10800), "1402");
        assert_eq!(frame["DH"], 91);
        assert_eq!(frame["A0"], 0);
        assert_eq!(frame["B0"], 91);
        assert!(stock_frame(None, Some(10800), 0)
            .values()
            .all(|level| *level == 0));
        assert!(stock_frame(
            Some(StockDisplay {
                enabled: false,
                level: 91
            }),
            Some(0),
            0
        )
        .values()
        .all(|level| *level == 0));
        assert_eq!(local_hhmm(30, -3600), "2300");
    }
}
