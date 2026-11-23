use iced::{Padding, Size};

/// NOTE: this was AI generated, but seemed to have given me pretty nice results.

/// The original window size the UI was designed at in Figma.
///
/// Every dimension is relative to this.
pub const DESIGN_SIZE: Size = Size::new(960.0, 524.0);

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    scale: f32,
}

impl Metrics {
    pub fn fit(available: Size) -> Self {
        let scale =
            (available.width / DESIGN_SIZE.width).min(available.height / DESIGN_SIZE.height);

        Self {
            scale: scale.max(f32::EPSILON),
        }
    }

    fn of(self, design_px: f32) -> f32 {
        design_px * self.scale
    }

    pub fn padding(self) -> Padding {
        Padding {
            top: self.of(12.0),
            bottom: self.of(12.0),
            left: self.of(24.0),
            right: self.of(24.0),
        }
    }

    pub fn header_gap(self) -> f32 {
        self.of(48.0)
    }

    pub fn controls_gap(self) -> f32 {
        self.of(32.0)
    }

    pub fn knob_size(self) -> f32 {
        self.of(128.0)
    }

    pub fn knob_title_height(self) -> f32 {
        self.of(20.0)
    }

    pub fn knob_title_gap(self) -> f32 {
        self.of(8.0)
    }

    pub fn knob_row_gap(self) -> f32 {
        self.of(32.0)
    }

    pub fn knob_column_gap(self) -> f32 {
        self.of(32.0)
    }

    pub fn section_label_gap(self) -> f32 {
        self.of(16.0)
    }

    pub fn section_label_indent(self) -> f32 {
        self.of(12.0)
    }

    pub fn logo_size(self) -> Size {
        Size::new(self.of(144.0), self.of(30.0))
    }

    pub fn logo_top(self) -> f32 {
        self.of(32.0)
    }

    pub fn label_text(self) -> f32 {
        self.of(14.0)
    }

    pub fn knob_title_text(self) -> f32 {
        self.of(16.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_size_is_unit_scale() {
        assert_eq!(Metrics::fit(DESIGN_SIZE).knob_size(), 128.0);
    }

    #[test]
    fn constrained_axis_drives_scale() {
        let wide = Metrics::fit(Size::new(DESIGN_SIZE.width * 4.0, DESIGN_SIZE.height * 2.0));
        assert_eq!(wide.knob_size(), 256.0);

        let tall = Metrics::fit(Size::new(DESIGN_SIZE.width / 2.0, DESIGN_SIZE.height * 3.0));
        assert_eq!(tall.knob_size(), 64.0);
    }

    #[test]
    fn zero_size_stays_positive() {
        assert!(Metrics::fit(Size::ZERO).knob_size() > 0.0);
    }
}
