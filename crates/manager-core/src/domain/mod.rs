use serde::{Deserialize, Serialize};

/// Persisted desktop window placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub schema_version: u32,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_maximized: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            schema_version: 1,
            width: 1120,
            height: 760,
            x: 100,
            y: 100,
            is_maximized: false,
        }
    }
}
