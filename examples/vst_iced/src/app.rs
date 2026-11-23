use iced::Padding;
use iced::alignment::Horizontal;
use iced::gradient;
use iced::widget::container::background;
use iced::widget::{Column, column, container, row, rule, space, svg, text};
use iced::{Center, Degrees, Element, Fill, Pixels, Shrink};

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
        .spacing(48);

        let grain_size = labeled_knob("Grain Size", self.state.grain_size, Message::GrainSize);
        let grains = labeled_knob("Grains", self.state.grains, Message::Grains);
        let feedback = labeled_knob("Feedback", self.state.feedback, Message::Feedback);
        let jitter = labeled_knob("Jitter", self.state.jitter, Message::Jitter);
        let dampening = labeled_knob("Dampening", self.state.dampening, Message::Dampening);
        let diffusion = labeled_knob("Diffusion", self.state.diffusion, Message::Diffusion);

        let grain_knobs = row![
            knob_column([grain_size, grains]),
            knob_column([feedback, jitter]),
            knob_column([dampening, diffusion])
        ]
        .spacing(32);

        let decay = labeled_knob("Decay", self.state.decay, Message::Decay);
        let reverb_mix = labeled_knob("Reverb Mix", self.state.reverb_mix, Message::ReverbMix);

        let wet_dry = labeled_knob("Mix", self.state.wet_dry, Message::WetDry);

        let divider = || {
            rule::vertical(1).style(|_| rule::Style {
                color: theme::DIVIDER,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            })
        };

        let reverb_knobs = knob_column([decay, reverb_mix]);

        // TODO: is there a better way of automatically spacing?
        //
        // I was kind of hoping for something like a .flex(), .justify_between()?
        let sections = row![
            section("Grain", grain_knobs),
            space::horizontal(),
            divider(),
            space::horizontal(),
            floor_section("Reverb", reverb_knobs),
            space::horizontal(),
            divider(),
            space::horizontal(),
            floor_section("Master", wet_dry),
            space::horizontal(),
        ]
        .height(Shrink);

        let backdrop = gradient::Linear::new(Degrees(13.0))
            .add_stop(0.3, theme::BACKGROUND_TOP)
            .add_stop(1.0, theme::BACKGROUND_BOTTOM);

        let logo = container(
            svg(svg::Handle::from_memory(theme::LOGO_SVG))
                .width(144)
                .height(30),
        )
        .width(Fill)
        .padding(Padding {
            top: 32.0,
            ..Padding::default()
        })
        .align_x(Horizontal::Right);

        let controls = column![header, sections].spacing(32);
        let layout = column![controls, space::vertical(), logo];

        let content = container(layout)
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
const KNOB_TITLE_HEIGHT: f32 = 20.0;
const KNOB_TITLE_GAP: f32 = 8.0;
const KNOB_ROW_GAP: f32 = 32.0;

fn knob_column<'a>(knobs: impl IntoIterator<Item = Element<'a, Message>>) -> Element<'a, Message> {
    Column::with_children(knobs).spacing(KNOB_ROW_GAP).into()
}

/// These are the groupings of different parameters by what area they effect.
///
/// For instance, grain, reverb, and master.
fn section<'a>(label: &'a str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![content.into(), section_label(label)]
        .spacing(SECTION_LABEL_GAP)
        .into()
}

/// A section that stretches to the row's height with its label pinned to the bottom.
///
/// The row needs at least one plain [`section`] to define that height, otherwise
/// every child is `Fill` in a `Shrink` row and the whole row collapses to zero.
fn floor_section<'a>(
    label: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        content.into(),
        space::vertical().height(Fill),
        section_label(label)
    ]
    .spacing(SECTION_LABEL_GAP / 2.0)
    .into()
}

const SECTION_LABEL_GAP: f32 = 16.0;

fn section_label<'a>(label: &'a str) -> Element<'a, Message> {
    container(
        text(label)
            .font(theme::SATOSHI)
            .color(theme::SECONDARY)
            .size(14),
    )
    .padding(Padding {
        left: 12.0,
        ..Padding::default()
    })
    .into()
}

fn labeled_knob<'a>(
    label: &'a str,
    value: f32,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    column![
        text(label)
            .font(theme::CHILLAX)
            .size(16)
            .line_height(Pixels(KNOB_TITLE_HEIGHT)),
        Knob::new(value, on_change).size(KNOB_SIZE)
    ]
    .spacing(KNOB_TITLE_GAP)
    .width(KNOB_SIZE)
    .align_x(Center)
    .into()
}
