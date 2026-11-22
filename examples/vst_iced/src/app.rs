use iced::Padding;
use iced::alignment::{Horizontal, Vertical};
use iced::border;
use iced::gradient;
use iced::keyboard;
use iced::mouse;
use iced::widget::container::background;
use iced::widget::{
    button, canvas, center, center_y, checkbox, column, container, pick_list, pin, responsive, row,
    rule, scrollable, space, stack, svg, text,
};
use iced::{
    Center, Degrees, Element, Fill, FillPortion, Font, Length, Point, Rectangle, Renderer, Shrink,
    Subscription, Theme, color,
};

use crate::knob::Knob;
use crate::theme;

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
        let header = row![
            text("Grain Sync").font(theme::SATOSHI).size(14),
            text("Delay Sync").font(theme::SATOSHI).size(14)
        ]
        .spacing(24);

        let grain_size = labeled_knob("Grain Size", self.state.grain_size, Message::GrainSize);
        let grains = labeled_knob("Grains", self.state.grains, Message::Grains);
        let feedback = labeled_knob("Feedback", self.state.feedback, Message::Feedback);
        let jitter = labeled_knob("Jitter", self.state.jitter, Message::Jitter);
        let dampening = labeled_knob("Dampening", self.state.dampening, Message::Dampening);
        let diffusion = labeled_knob("Diffusion", self.state.diffusion, Message::Diffusion);

        let grain_knobs = row![
            column![grain_size, grains].spacing(32),
            column![feedback, jitter].spacing(32),
            column![dampening, diffusion].spacing(32)
        ]
        .spacing(32);

        let knobs = row![grain_knobs].spacing(24);

        let backdrop = gradient::Linear::new(Degrees(13.0))
            .add_stop(0.3, theme::BACKGROUND_TOP)
            .add_stop(1.0, theme::BACKGROUND_BOTTOM);

        let logo = container(
            svg(svg::Handle::from_memory(theme::LOGO_SVG))
                .width(144)
                .height(30),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Right)
        .align_y(Vertical::Bottom);

        let layers = stack![column![header, knobs].spacing(24), logo]
            .width(Fill)
            .height(Fill);

        let content = container(layers)
            .width(Fill)
            .height(Fill)
            .padding(Padding {
                top: 12.0,
                bottom: 12.0,
                left: 24.0,
                right: 24.0,
            })
            .style(move |_| background(backdrop));

        container(content).into()
    }
}

const KNOB_SIZE: f32 = 128.0;

fn labeled_knob<'a>(
    label: &'a str,
    value: f32,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    column![
        text(label).font(theme::CHILLAX).size(16),
        Knob::new(value, on_change).size(KNOB_SIZE)
    ]
    .spacing(8)
    .align_x(Center)
    .into()
}
