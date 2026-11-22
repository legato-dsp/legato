use iced::border;
use iced::gradient;
use iced::keyboard;
use iced::mouse;
use iced::widget::container::background;
use iced::widget::{
    button, canvas, center, center_y, checkbox, column, container, pick_list, pin, responsive, row,
    rule, scrollable, space, stack, text,
};
use iced::{
    Center, Degrees, Element, Fill, FillPortion, Font, Length, Point, Rectangle, Renderer, Shrink,
    Subscription, Theme, color,
};

use crate::knob::Knob;

pub enum Message {
    GrainSize(f32),
    Grains(f32),
    Feedback(f32),
    Jitter(f32),
    Dampening(f32),
    Diffusion(f32),
    Decay(f32),
    ReverbMix(f32),
    WetDry(f32),
}

struct AppState {
    grain_size: f32,
    grains: f32,
    feedback: f32,
    jitter: f32,
    dampening: f32,
    diffusion: f32,
    decay: f32,
    reverb_mix: f32,
    wet_dry: f32,
}

pub struct App {
    state: AppState,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState {
                grain_size: 400.,
                grains: 4.0,
                feedback: 0.3,
                jitter: 0.2,
                dampening: 0.4,
                diffusion: 0.3,
                decay: 0.4,
                reverb_mix: 0.3,
                wet_dry: 0.5,
            },
        }
    }
    pub fn update(&mut self, message: Message) {
        match message {
            Message::GrainSize(val) => self.state.grain_size = val,
            Message::Grains(val) => self.state.grains = val,
            Message::Feedback(val) => self.state.feedback = val,
            Message::Jitter(val) => self.state.jitter = val,
            Message::Dampening(val) => self.state.dampening = val,
            Message::Diffusion(val) => self.state.diffusion = val,
            Message::Decay(val) => self.state.decay = val,
            Message::ReverbMix(val) => self.state.reverb_mix = val,
            Message::WetDry(val) => self.state.wet_dry = val,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let header = row![text("Grain Sync"), text("Delay Sync")].spacing(12);

        let grain_size = Knob::new(self.state.grain_size, |x| Message::GrainSize(x));
        let grains = Knob::new(self.state.grains, |x| Message::Grains(x));
        let feedback = Knob::new(self.state.feedback, |x| Message::Feedback(x));
        let jitter = Knob::new(self.state.jitter, |x| Message::Jitter(x));
        let dampening = Knob::new(self.state.dampening, |x| Message::Dampening(x));
        let diffusion = Knob::new(self.state.diffusion, |x| Message::Diffusion(x));

        let grain_knobs = row![
            column![grain_size, grains],
            column![feedback, jitter],
            column![dampening, diffusion]
        ]
        .spacing(12);

        let knobs = row![grain_knobs].spacing(32);

        let backdrop = gradient::Linear::new(Degrees(32.0))
            .add_stop(0.0, color!(0x110C0E))
            .add_stop(1.0, color!(0x10111C));

        let content = container(column![header, knobs].padding(12).spacing(24))
            .width(Fill)
            .height(Fill)
            .style(move |_| background(backdrop));

        container(content).into()
    }
}
