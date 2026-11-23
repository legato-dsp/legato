use iced::alignment::Horizontal;
use iced::widget::container::background;
use iced::widget::{Column, column, container, responsive, row, rule, space, svg, text};
use iced::{Center, Degrees, Element, Fill, Pixels, Shrink};
use iced::{gradient, padding};

use crate::knob::Knob;
use crate::metrics::Metrics;
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
        responsive(move |size| self.view_at(Metrics::fit(size))).into()
    }

    fn view_at(&self, m: Metrics) -> Element<'_, Message> {
        let header = row![
            text("Grain Sync").font(theme::SATOSHI).size(m.label_text()),
            text("Delay Sync").font(theme::SATOSHI).size(m.label_text())
        ]
        .spacing(m.header_gap());

        let grain_size = labeled_knob(m, "Grain Size", self.state.grain_size, Message::GrainSize);
        let grains = labeled_knob(m, "Grains", self.state.grains, Message::Grains);
        let feedback = labeled_knob(m, "Feedback", self.state.feedback, Message::Feedback);
        let jitter = labeled_knob(m, "Jitter", self.state.jitter, Message::Jitter);
        let dampening = labeled_knob(m, "Dampening", self.state.dampening, Message::Dampening);
        let diffusion = labeled_knob(m, "Diffusion", self.state.diffusion, Message::Diffusion);

        let grain_knobs = row![
            knob_column(m, [grain_size, grains]),
            knob_column(m, [feedback, jitter]),
            knob_column(m, [dampening, diffusion])
        ]
        .spacing(m.knob_column_gap());

        let decay = labeled_knob(m, "Decay", self.state.decay, Message::Decay);
        let reverb_mix = labeled_knob(m, "Reverb Mix", self.state.reverb_mix, Message::ReverbMix);

        let wet_dry = labeled_knob(m, "Mix", self.state.wet_dry, Message::WetDry);

        let divider = || {
            rule::vertical(1).style(|_| rule::Style {
                color: theme::DIVIDER,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            })
        };

        let reverb_knobs = knob_column(m, [decay, reverb_mix]);

        // TODO: is there a better way of automatically spacing?
        //
        // I was kind of hoping for something like a .flex(), .justify_between()?
        let sections = row![
            section(m, "Grain", grain_knobs),
            space::horizontal(),
            divider(),
            space::horizontal(),
            floor_section(m, "Reverb", reverb_knobs),
            space::horizontal(),
            divider(),
            space::horizontal(),
            floor_section(m, "Master", wet_dry),
            space::horizontal(),
        ]
        .height(Shrink);

        let backdrop = gradient::Linear::new(Degrees(13.0))
            .add_stop(0.3, theme::BACKGROUND_TOP)
            .add_stop(1.0, theme::BACKGROUND_BOTTOM);

        let logo_size = m.logo_size();
        let logo = container(
            svg(svg::Handle::from_memory(theme::LOGO_SVG))
                .width(logo_size.width)
                .height(logo_size.height),
        )
        .width(Fill)
        .padding(padding::top(m.logo_top()))
        .align_x(Horizontal::Right);

        let controls = column![header, sections].spacing(m.controls_gap());
        let layout = column![controls, space::vertical(), logo];

        container(layout)
            .width(Fill)
            .height(Fill)
            .padding(m.padding())
            .style(move |_| background(backdrop))
            .into()
    }
}

fn knob_column<'a>(
    m: Metrics,
    knobs: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    Column::with_children(knobs)
        .spacing(m.knob_row_gap())
        .into()
}

/// These are the groupings of different parameters by what area they effect.
///
/// For instance, grain, reverb, and master.
fn section<'a>(
    m: Metrics,
    label: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![content.into(), section_label(m, label)]
        .spacing(m.section_label_gap())
        .into()
}

/// A section that stretches to the row's height with its label pinned to the bottom.
///
/// The row needs at least one plain [`section`] to define that height, otherwise
/// every child is `Fill` in a `Shrink` row and the whole row collapses to zero.
fn floor_section<'a>(
    m: Metrics,
    label: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        content.into(),
        space::vertical().height(Fill),
        section_label(m, label)
    ]
    .spacing(m.section_label_gap() / 2.0)
    .into()
}

fn section_label<'a>(m: Metrics, label: &'a str) -> Element<'a, Message> {
    container(
        text(label)
            .font(theme::SATOSHI)
            .color(theme::SECONDARY)
            .size(m.label_text()),
    )
    .padding(padding::left(m.section_label_indent()))
    .into()
}

fn labeled_knob<'a>(
    m: Metrics,
    label: &'a str,
    value: f32,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    column![
        text(label)
            .font(theme::CHILLAX)
            .size(m.knob_title_text())
            .line_height(Pixels(m.knob_title_height())),
        Knob::new(value, on_change).size(m.knob_size())
    ]
    .spacing(m.knob_title_gap())
    .width(m.knob_size())
    .align_x(Center)
    .into()
}
