use iced::font::Weight;
use iced::{Color, Font};

pub const BACKGROUND_TOP: Color = Color::from_rgb8(0x11, 0x0C, 0x0E);
pub const BACKGROUND_BOTTOM: Color = Color::from_rgb8(0x10, 0x11, 0x1C);

pub const KNOB_RING_HIGHLIGHT: Color = Color::from_rgb8(167, 240, 255);
pub const KNOB_RING_SHADOW: Color = Color::from_rgba8(52, 40, 57, 0.51);
pub const KNOB_INDICATOR: Color = Color::WHITE;

/// Font required for building.
///
/// Note: For licensing reasons, I am not including this in the source repository.
/// You can find these fonts on fontshare.
pub const SATOSHI_BYTES: &[u8] = include_bytes!("../assets/fonts/Satoshi-Light.otf");
pub const CHILLAX_BYTES: &[u8] = include_bytes!("../assets/fonts/Chillax-Regular.otf");

pub const SATOSHI: Font = Font {
    weight: Weight::Light,
    ..Font::with_name("Satoshi")
};
pub const CHILLAX: Font = Font::with_name("Chillax");

pub const LOGO_SVG: &[u8] = include_bytes!("../assets/logo/logo.svg");
