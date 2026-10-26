use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub enum ColorOrder {
    Rgb,
    Bgr,
}

#[derive(Debug, Clone)]
pub struct AdapterProfile {
    pub id: String,
    pub model: String,
    pub led_count: usize,
    pub color_order: ColorOrder,
    pub sensors_proximity: bool,
    pub sensors_ambient_light: bool,
}

pub trait LedBackend: Send + Sync {
    fn show(&self, frame: &[u8]) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockCommand {
    Time { text: String, brightness: u8 },
    Segments(HashMap<String, u8>),
    Off,
}

pub trait ClockBackend: Send + Sync {
    fn segments(&self) -> Vec<String>;
    fn apply(&self, command: ClockCommand) -> Result<()>;
}

pub trait DeviceAdapter: Send + Sync {
    fn profile(&self) -> AdapterProfile;
    fn create_led_backend(&self) -> Result<Box<dyn LedBackend>>;
    fn create_clock_backend(&self) -> Option<Box<dyn ClockBackend>> {
        None
    }
}
