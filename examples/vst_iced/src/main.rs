// use nice_plug::prelude::*;

// use gain_iced::Gain;

use legato_grain_delay::app::App;
use legato_grain_delay::metrics::DESIGN_SIZE;
use legato_grain_delay::theme;

pub fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .font(theme::SATOSHI_BYTES)
        .font(theme::CHILLAX_BYTES)
        .window_size(DESIGN_SIZE)
        .run()
}

// fn main(){
//     nice_export_standalone::<Gain>();
// }
