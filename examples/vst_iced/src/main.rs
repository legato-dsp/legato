// use nice_plug::prelude::*;

// use gain_iced::Gain;

use gain_iced::app::App;

pub fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .window_size(iced::Size::new(960.0, 480.0))
        .run()
}

// fn main(){
//     nice_export_standalone::<Gain>();
// }
