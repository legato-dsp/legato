// use nice_plug::prelude::*;

// use gain_iced::Gain;

use gain_iced::knob::Knob;

pub fn main() -> iced::Result {
    iced::application(Knob::new, Knob::update, Knob::view).run()
}

// fn main(){
//     nice_export_standalone::<Gain>();
// }
