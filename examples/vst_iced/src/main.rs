// use nice_plug::prelude::*;

// use gain_iced::Gain;

use legato_grain_delay::app::App;
use legato_grain_delay::theme;

pub fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .font(theme::SATOSHI_BYTES)
        .font(theme::CHILLAX_BYTES)
        .window_size(iced::Size::new(960.0, 480.0))
        .run()
}

// fn main(){
//     nice_export_standalone::<Gain>();
// }
