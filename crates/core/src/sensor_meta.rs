use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorSource {
    Nvml,
    Nvapi,
    #[default]
    Unknown,
}

impl SensorSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nvml => "nvml",
            Self::Nvapi => "nvapi",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorQuality {
    Live,
    Nominal,
    #[default]
    Unavailable,
}
