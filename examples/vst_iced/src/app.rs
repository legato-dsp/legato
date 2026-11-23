use std::sync::Arc;

use nice_plug::prelude::{BoolParam, EnumParam, FloatParam, IntParam, Param, ParamSetter};
use nice_plug_iced::iced::alignment::Horizontal;
use nice_plug_iced::iced::widget::container::background;
use nice_plug_iced::iced::widget::text::IntoFragment;
use nice_plug_iced::iced::widget::{
    Column, button, column, container, responsive, row, rule, space, svg, text,
};
use nice_plug_iced::iced::{
    Center, Degrees, Element, Fill, Pixels, Shrink, Subscription, gradient, padding, poll_events,
    window_resized,
};
use nice_plug_iced::{IcedNiceContext, PersistentState};

use crate::knob::{Gesture, Knob};
use crate::metrics::Metrics;
use crate::params::{Division, GrainDelayParams};
use crate::theme;

#[derive(Debug, Clone, Copy)]
pub enum Control {
    DelayTime,
    GrainSize,
    Grains,
    Pitch,
    Shape,
    Feedback,
    Jitter,
    Damping,
    Diffusion,
    Decay,
    ReverbMix,
    Mix,
}

#[derive(Debug, Clone, Copy)]
pub enum Switch {
    DelaySync,
    GrainSync,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    /// Redraws after the host changes a param.
    Poll,
    WindowResized,
    Knob(Control, Gesture),
    Toggle(Switch),
}

enum KnobParam<'a> {
    Float(&'a FloatParam),
    Int(&'a IntParam),
    Division(&'a EnumParam<Division>),
}

impl KnobParam<'_> {
    fn normalized(&self) -> f32 {
        match self {
            Self::Float(p) => p.unmodulated_normalized_value(),
            Self::Int(p) => p.unmodulated_normalized_value(),
            Self::Division(p) => p.unmodulated_normalized_value(),
        }
    }

    fn apply(&self, setter: &ParamSetter, gesture: Gesture) {
        match self {
            Self::Float(p) => apply(setter, *p, gesture),
            Self::Int(p) => apply(setter, *p, gesture),
            Self::Division(p) => apply(setter, *p, gesture),
        }
    }
}

fn apply<P: Param>(setter: &ParamSetter, param: &P, gesture: Gesture) {
    match gesture {
        Gesture::Grab => setter.begin_set_parameter(param),
        Gesture::Drag(value) => setter.set_parameter_normalized(param, value),
        Gesture::Release => setter.end_set_parameter(param),
    }
}

pub struct App {
    params: PersistentState<Arc<GrainDelayParams>>,
    ctx: IcedNiceContext,
}

impl App {
    pub fn new(params: PersistentState<Arc<GrainDelayParams>>, ctx: IcedNiceContext) -> Self {
        Self { params, ctx }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            poll_events().map(|_| Message::Poll),
            window_resized().map(|_| Message::WindowResized),
        ])
    }

    pub fn update(&mut self, message: Message) {
        let setter = self.ctx.nice_context.param_setter();
        match message {
            Message::Poll => {}
            Message::WindowResized => self.ctx.sync_window_size(),
            Message::Knob(control, gesture) => self.knob_param(control).apply(&setter, gesture),
            Message::Toggle(switch) => {
                let param = self.switch_param(switch);
                setter.begin_set_parameter(param);
                setter.set_parameter(param, !param.value());
                setter.end_set_parameter(param);
            }
        }
    }

    fn knob_param(&self, control: Control) -> KnobParam<'_> {
        let p = &**self.params;
        match control {
            Control::DelayTime if p.delay_sync.value() => KnobParam::Division(&p.delay_division),
            Control::DelayTime => KnobParam::Float(&p.delay_time),
            Control::GrainSize if p.sync.value() => KnobParam::Division(&p.division),
            Control::GrainSize => KnobParam::Float(&p.grain_size),
            Control::Grains => KnobParam::Int(&p.grains),
            Control::Pitch => KnobParam::Float(&p.pitch),
            Control::Shape => KnobParam::Float(&p.shape),
            Control::Feedback => KnobParam::Float(&p.feedback),
            Control::Jitter => KnobParam::Float(&p.jitter),
            Control::Damping => KnobParam::Float(&p.damping),
            Control::Diffusion => KnobParam::Float(&p.diffusion),
            Control::Decay => KnobParam::Float(&p.reverb_decay),
            Control::ReverbMix => KnobParam::Float(&p.reverb_mix),
            Control::Mix => KnobParam::Float(&p.mix),
        }
    }

    fn switch_param(&self, switch: Switch) -> &BoolParam {
        match switch {
            Switch::DelaySync => &self.params.delay_sync,
            Switch::GrainSync => &self.params.sync,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        responsive(move |size| self.view_at(Metrics::fit(size))).into()
    }

    fn view_at(&self, m: Metrics) -> Element<'_, Message> {
        let p = &**self.params;

        let header = row![
            self.toggle(m, "Grain Sync", Switch::GrainSync),
            self.toggle(m, "Delay Sync", Switch::DelaySync)
        ]
        .spacing(m.header_gap());

        let knob = |label: &'static str, control| self.labeled_knob(m, label, control);

        let grain_size = match p.sync.value() {
            true => self.labeled_knob(m, format!("Grain {}", p.division), Control::GrainSize),
            false => knob("Grain Size", Control::GrainSize),
        };
        let delay = match p.delay_sync.value() {
            true => self.labeled_knob(m, format!("Delay {}", p.delay_division), Control::DelayTime),
            false => knob("Delay", Control::DelayTime),
        };

        let grain_knobs = row![
            knob_column(m, [grain_size, knob("Grains", Control::Grains)]),
            knob_column(
                m,
                [knob("Pitch", Control::Pitch), knob("Shape", Control::Shape)]
            ),
            knob_column(
                m,
                [
                    knob("Feedback", Control::Feedback),
                    knob("Jitter", Control::Jitter)
                ]
            ),
            knob_column(
                m,
                [
                    knob("Dampening", Control::Damping),
                    knob("Diffusion", Control::Diffusion)
                ]
            ),
        ]
        .spacing(m.knob_column_gap());

        let reverb_knobs = knob_column(
            m,
            [
                knob("Decay", Control::Decay),
                knob("Reverb Mix", Control::ReverbMix),
            ],
        );
        let master_knobs = knob_column(m, [delay, knob("Mix", Control::Mix)]);

        let divider = || {
            rule::vertical(1).style(|_| rule::Style {
                color: theme::DIVIDER,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            })
        };

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
            floor_section(m, "Master", master_knobs),
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

    fn toggle<'a>(&self, m: Metrics, label: &'a str, switch: Switch) -> Element<'a, Message> {
        let color = match self.switch_param(switch).value() {
            true => theme::PRIMARY,
            false => theme::SECONDARY,
        };
        button(
            text(label)
                .font(theme::SATOSHI)
                .size(m.label_text())
                .color(color),
        )
        .padding(0)
        .style(|_, _| button::Style::default())
        .on_press(Message::Toggle(switch))
        .into()
    }

    fn labeled_knob<'a>(
        &self,
        m: Metrics,
        label: impl IntoFragment<'a>,
        control: Control,
    ) -> Element<'a, Message> {
        let value = self.knob_param(control).normalized();
        column![
            text(label)
                .font(theme::CHILLAX)
                .color(theme::PRIMARY)
                .size(m.knob_title_text())
                .line_height(Pixels(m.knob_title_height())),
            Knob::new(value, move |gesture| Message::Knob(control, gesture)).size(m.knob_size())
        ]
        .spacing(m.knob_title_gap())
        .width(m.knob_size())
        .align_x(Center)
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
