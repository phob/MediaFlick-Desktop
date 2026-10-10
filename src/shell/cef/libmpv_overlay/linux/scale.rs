//! CEF uses device-independent coordinates; X11 reports drawable pixels.
//! Xft.dpi determines layout and input. On Wayland, raster density is capped at
//! the physical output density; libmpv stretches the bitmap to X11 coordinates.
use cef::Rect;
use x11rb::connection::Connection;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Scale {
    desktop: f64,
    raster: f64,
}

impl Default for Scale {
    fn default() -> Self {
        Self {
            desktop: 1.0,
            raster: 1.0,
        }
    }
}

impl Scale {
    pub fn query(conn: &impl Connection) -> Result<Self, x11rb::errors::ReplyError> {
        let db = x11rb::resource_manager::new_from_default(conn)?;
        Ok(Self::from_dpi(
            db.get_value::<f64>("Xft.dpi", "").ok().flatten(),
        ))
    }

    fn from_dpi(dpi: Option<f64>) -> Self {
        let desktop = dpi
            .filter(|v| v.is_finite() && (48.0..=768.0).contains(v))
            .unwrap_or(96.0)
            / 96.0;
        Self {
            desktop,
            raster: desktop,
        }
    }

    pub fn with_native_density(self, density: Option<f64>) -> Self {
        let raster = density
            .filter(|v| v.is_finite() && (0.5..=8.0).contains(v))
            .map_or(self.desktop, |density| density.min(self.desktop));
        Self {
            desktop: self.desktop,
            raster,
        }
    }

    pub fn factor(self) -> f32 {
        self.raster as f32
    }

    pub fn logical(self, pixels: i32) -> i32 {
        (f64::from(pixels) / self.desktop).floor() as i32
    }

    pub fn logical_extent(self, pixels: u16) -> i32 {
        (f64::from(pixels) / self.desktop).ceil().max(1.0) as i32
    }

    pub fn physical(self, logical: i32) -> i32 {
        (f64::from(logical) * self.desktop).round() as i32
    }

    pub fn raster_extent(self, pixels: u16) -> u16 {
        (f64::from(self.logical_extent(pixels)) * self.raster).ceil() as u16
    }

    pub fn raster_rect(self, rect: &Rect) -> Rect {
        Rect {
            x: (f64::from(rect.x) * self.raster).round() as i32,
            y: (f64::from(rect.y) * self.raster).round() as i32,
            width: (f64::from(rect.width) * self.raster).ceil() as i32,
            height: (f64::from(rect.height) * self.raster).ceil() as i32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_invalid_dpi_uses_unscaled_coordinates() {
        for dpi in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(0.0),
            Some(-96.0),
            Some(10000.0),
        ] {
            assert_eq!(Scale::from_dpi(dpi), Scale::default());
        }
    }
}
