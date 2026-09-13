use manager_core::domain::WindowGeometry;
use manager_core::ports::StateRepository;
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPlacement {
    Apply {
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        is_maximized: bool,
    },
    FallbackCenter {
        width: u32,
        height: u32,
    },
}

pub fn calculate_window_placement(
    saved: Option<WindowGeometry>,
    monitors: &[MonitorBounds],
) -> ResolvedPlacement {
    let Some(saved) = saved else {
        return ResolvedPlacement::FallbackCenter {
            width: 1120,
            height: 760,
        };
    };

    let is_valid_size =
        saved.width >= 800 && saved.height >= 600 && saved.width <= 10000 && saved.height <= 10000;

    if !is_valid_size {
        return ResolvedPlacement::FallbackCenter {
            width: 1120,
            height: 760,
        };
    }

    let mut is_on_any_monitor = false;
    for m in monitors {
        let min_x = m.x;
        let max_x = m.x + m.width as i32;
        let min_y = m.y;
        let max_y = m.y + m.height as i32;

        if saved.x >= min_x && saved.x < max_x && saved.y >= min_y && saved.y < max_y {
            is_on_any_monitor = true;
            break;
        }
    }

    if is_on_any_monitor {
        ResolvedPlacement::Apply {
            width: saved.width,
            height: saved.height,
            x: saved.x,
            y: saved.y,
            is_maximized: saved.is_maximized,
        }
    } else {
        ResolvedPlacement::FallbackCenter {
            width: 1120,
            height: 760,
        }
    }
}

pub fn restore_window_geometry(window: &WebviewWindow, repo: &dyn StateRepository) {
    let saved = repo.get_window_geometry().ok().flatten();
    let monitors: Vec<MonitorBounds> = window
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| MonitorBounds {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
        })
        .collect();

    match calculate_window_placement(saved, &monitors) {
        ResolvedPlacement::Apply {
            width,
            height,
            x,
            y,
            is_maximized,
        } => {
            let _ = window.set_size(PhysicalSize::new(width, height));
            let _ = window.set_position(PhysicalPosition::new(x, y));
            if is_maximized {
                let _ = window.maximize();
            }
        }
        ResolvedPlacement::FallbackCenter { width, height } => {
            let _ = window.set_size(PhysicalSize::new(width, height));
            let _ = window.center();
        }
    }
}

pub fn persist_window_geometry(window: &WebviewWindow, repo: &dyn StateRepository) {
    if let (Ok(size), Ok(pos), Ok(is_max)) = (
        window.inner_size(),
        window.outer_position(),
        window.is_maximized(),
    ) {
        let geom = WindowGeometry {
            schema_version: 1,
            width: size.width,
            height: size.height,
            x: pos.x,
            y: pos.y,
            is_maximized: is_max,
        };
        let _ = repo.save_window_geometry(&geom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_window_placement_no_saved_geometry() {
        let monitors = vec![MonitorBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let placement = calculate_window_placement(None, &monitors);
        assert_eq!(
            placement,
            ResolvedPlacement::FallbackCenter {
                width: 1120,
                height: 760
            }
        );
    }

    #[test]
    fn test_calculate_window_placement_valid_single_monitor() {
        let monitors = vec![MonitorBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let saved = WindowGeometry {
            schema_version: 1,
            width: 1200,
            height: 800,
            x: 200,
            y: 150,
            is_maximized: false,
        };
        let placement = calculate_window_placement(Some(saved), &monitors);
        assert_eq!(
            placement,
            ResolvedPlacement::Apply {
                width: 1200,
                height: 800,
                x: 200,
                y: 150,
                is_maximized: false,
            }
        );
    }

    #[test]
    fn test_calculate_window_placement_negative_coordinates_left_monitor() {
        let monitors = vec![
            MonitorBounds {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            MonitorBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];
        let saved = WindowGeometry {
            schema_version: 1,
            width: 1024,
            height: 768,
            x: -1500,
            y: 200,
            is_maximized: true,
        };
        let placement = calculate_window_placement(Some(saved), &monitors);
        assert_eq!(
            placement,
            ResolvedPlacement::Apply {
                width: 1024,
                height: 768,
                x: -1500,
                y: 200,
                is_maximized: true,
            }
        );
    }

    #[test]
    fn test_calculate_window_placement_disconnected_monitor_fallback() {
        // Saved coordinate was on external monitor (-1920, 0), but now only single monitor (0, 0) is connected
        let monitors = vec![MonitorBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let saved = WindowGeometry {
            schema_version: 1,
            width: 1200,
            height: 800,
            x: -1500,
            y: 200,
            is_maximized: false,
        };
        let placement = calculate_window_placement(Some(saved), &monitors);
        assert_eq!(
            placement,
            ResolvedPlacement::FallbackCenter {
                width: 1120,
                height: 760
            }
        );
    }

    #[test]
    fn test_calculate_window_placement_corrupted_dimensions() {
        let monitors = vec![MonitorBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];

        // Tiny width / height < 800x600
        let saved_tiny = WindowGeometry {
            schema_version: 1,
            width: 200,
            height: 100,
            x: 50,
            y: 50,
            is_maximized: false,
        };
        assert_eq!(
            calculate_window_placement(Some(saved_tiny), &monitors),
            ResolvedPlacement::FallbackCenter {
                width: 1120,
                height: 760
            }
        );

        // Huge width > 10000
        let saved_huge = WindowGeometry {
            schema_version: 1,
            width: 50000,
            height: 1080,
            x: 50,
            y: 50,
            is_maximized: false,
        };
        assert_eq!(
            calculate_window_placement(Some(saved_huge), &monitors),
            ResolvedPlacement::FallbackCenter {
                width: 1120,
                height: 760
            }
        );
    }
}
