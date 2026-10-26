use anyhow::Result;

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
}

pub trait LedBackend: Send + Sync {
    fn show(&self, frame: &[u8]) -> Result<()>;
}

pub trait DeviceAdapter: Send + Sync {
    fn profile(&self) -> AdapterProfile;
    fn create_led_backend(&self) -> Result<Box<dyn LedBackend>>;
}
