use std::f32::consts::PI;

use nice_plug_iced::iced::mouse;
use nice_plug_iced::iced::widget::canvas::Gradient;
use nice_plug_iced::iced::widget::canvas::{LineCap, Path, Stroke, stroke};
use nice_plug_iced::iced::widget::{Action, canvas};
use nice_plug_iced::iced::{Element, Event};

use crate::theme;
use nice_plug_iced::iced::{Point, Rectangle, Renderer, Theme};

/// Vertical drag distance, in knob heights, that sweeps the full value range.
const DRAG_RANGE: f32 = 200.0 / 128.0;

/// Ring stroke width as a fraction of the knob's half-extent.
const RING_WIDTH: f32 = 1.0 / 16.0;
/// Indicator dot radius as a fraction of the knob's half-extent.
const INDICATOR_RADIUS: f32 = 1.0 / 32.0;
/// How far the indicator orbits outside the ring, as a multiple of the ring radius.
const INDICATOR_ARM_RADIUS: f32 = 1.3;

/// A basic radial knob. The value is owned by the app and passed in each view.
///
/// This decision was made in order to keep the Knob in-sync with any changes
/// that could occur from say host automation.
pub struct Knob<F> {
    /// The normalized value from [0, 1] for the parameter
    value: f32,
    on_gesture: F,
    /// Side length of the knob's square bounds, in logical pixels
    size: f32,
}

/// Grab and Release bracket a drag so the host can group it into one automation edit.
#[derive(Debug, Clone, Copy)]
pub enum Gesture {
    Grab,
    Drag(f32),
    Release,
}

impl<F> Knob<F> {
    pub const DEFAULT_SIZE: f32 = 96.0;

    pub fn new(value: f32, on_gesture: F) -> Self {
        Knob {
            value: value.clamp(0.0, 1.0),
            on_gesture,
            size: Self::DEFAULT_SIZE,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

#[derive(Default)]
pub struct DragState {
    /// If it is `None`, then we have not yet pressed down with our cursor.
    drag: Option<Drag>,
}

struct Drag {
    last_y: f32,
    /// Tracked here rather than read back from `Knob::value`, as stepped params
    /// snap and would otherwise swallow small movements.
    value: f32,
}

impl<Message, F: Fn(Gesture) -> Message> canvas::Program<Message> for Knob<F> {
    type State = DragState;

    fn update(
        &self,
        state: &mut DragState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(event) = event else {
            return None;
        };

        match event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let pos = cursor.position_over(bounds)?;
                state.drag = Some(Drag {
                    last_y: pos.y,
                    value: self.value,
                });
                Some(Action::publish((self.on_gesture)(Gesture::Grab)).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                state.drag.take()?;
                Some(Action::publish((self.on_gesture)(Gesture::Release)).and_capture())
            }
            mouse::Event::CursorMoved { position } => {
                let drag = state.drag.as_mut()?;
                let delta = (drag.last_y - position.y) / (bounds.height * DRAG_RANGE);
                drag.last_y = position.y;
                drag.value = (drag.value + delta).clamp(0.0, 1.0);

                Some(Action::publish((self.on_gesture)(Gesture::Drag(drag.value))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &DragState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        let center = frame.center();
        let half = frame.width().min(frame.height()) / 2.0;
        let indicator_radius = half * INDICATOR_RADIUS;

        let arm = half - indicator_radius;
        let radius = arm / INDICATOR_ARM_RADIUS;

        let border = Path::circle(center, radius);

        let indicator_phase = indicator_angle(self.value);
        let indicator_pos = Point::new(
            indicator_phase.cos() * arm + center.x,
            indicator_phase.sin() * arm + center.y,
        );

        frame.fill(
            &Path::circle(indicator_pos, indicator_radius),
            theme::KNOB_INDICATOR,
        );

        let ring = canvas::gradient::Linear::new(
            Point::new(center.x, center.y - radius),
            Point::new(center.x, center.y + radius),
        )
        .add_stop(0.0, theme::KNOB_RING_HIGHLIGHT)
        .add_stop(1.0, theme::KNOB_RING_SHADOW);

        frame.stroke(
            &border,
            Stroke {
                width: half * RING_WIDTH,
                style: stroke::Style::Gradient(Gradient::Linear(ring)),
                line_cap: LineCap::Round,
                ..Stroke::default()
            },
        );

        vec![frame.into_geometry()]
    }
}

impl<'a, Message: 'a, F: Fn(Gesture) -> Message + 'a> From<Knob<F>> for Element<'a, Message> {
    fn from(knob: Knob<F>) -> Self {
        let size = knob.size;
        canvas(knob).width(size).height(size).into()
    }
}

// Get the angle of the indicator with a given normalized phase
// from 0 to 1. We start with an offset to keep it symetric,
// and we aren't using a complete 2 pi phase range as we want
// it more like a tachometer or plugin.
fn indicator_angle(normalized: f32) -> f32 {
    0.75 * PI + normalized * 1.5 * PI
}
