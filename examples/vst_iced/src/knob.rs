use std::f32::consts::PI;

use iced::mouse;
use iced::widget::canvas::Gradient;
use iced::widget::canvas::{LineCap, Path, Stroke, stroke};
use iced::widget::{Action, canvas};
use iced::{Element, Event};

use crate::theme;
use iced::{Point, Rectangle, Renderer, Theme};

const SENSITIVITY: f32 = 1.0 / 200.0;

/// A basic radial knob. The value is owned by the app and passed in each view.
///
/// This decision was made in order to keep the Knob in-sync with any changes
/// that could occur from say host automation.
pub struct Knob<Message> {
    /// The normalized value from [0, 1] for the parameter
    value: f32,
    on_change: fn(f32) -> Message,
    /// Side length of the knob's square bounds, in logical pixels
    size: f32,
}

impl<Message> Knob<Message> {
    pub const DEFAULT_SIZE: f32 = 96.0;

    pub fn new(value: f32, on_change: fn(f32) -> Message) -> Self {
        Knob {
            value: value.clamp(0.0, 1.0),
            on_change,
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
    /// We do need owned state for the last position our cursor was in.
    ///
    /// If it is `None`, then we have not yet pressed down with our cursor.
    last_pos: Option<Point>,
}

impl<Message> canvas::Program<Message> for Knob<Message> {
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
                state.last_pos = Some(pos);
                Some(Action::capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                // Note: Take the existing state value. This is important,
                // as we only publish cursor moving events when we do have
                // a last_pos. So us removing this means we stop, and are
                // effectively not dragging anymore.
                state.last_pos.take()?;
                Some(Action::capture())
            }
            mouse::Event::CursorMoved { position } => match state.last_pos.as_mut() {
                // If we do have a last set position, then we have already clicked
                // and started the drag.
                Some(last) => {
                    let delta = (last.y - position.y) * SENSITIVITY;
                    *last = *position;
                    let value = (self.value + delta).clamp(0.0, 1.0);

                    Some(Action::publish((self.on_change)(value)).and_capture())
                }
                // Otherwise, we haven't started dragging yet, and we just ignore
                None => return None,
            },
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

        const WIDTH: f32 = 4.0;
        const INDICATOR_ARM_RADIUS: f32 = 1.3;
        const INDICATOR_RADIUS: f32 = 2.0;

        let center = frame.center();
        let half = frame.width().min(frame.height()) / 2.0;

        let arm = half - INDICATOR_RADIUS;
        let radius = arm / INDICATOR_ARM_RADIUS;

        let border = Path::circle(center, radius);

        let indicator_phase = indicator_angle(self.value);
        let indicator_pos = Point::new(
            indicator_phase.cos() * arm + center.x,
            indicator_phase.sin() * arm + center.y,
        );

        frame.fill(
            &Path::circle(indicator_pos, INDICATOR_RADIUS),
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
                width: WIDTH,
                style: stroke::Style::Gradient(Gradient::Linear(ring)),
                line_cap: LineCap::Round,
                ..Stroke::default()
            },
        );

        vec![frame.into_geometry()]
    }
}

impl<'a, Message: 'a> From<Knob<Message>> for Element<'a, Message> {
    fn from(knob: Knob<Message>) -> Self {
        let size = knob.size;
        canvas(knob).width(size).height(size).into()
    }
}

// Get the angle of the indicator with a given normalized phase
// from 0 to 1. We start with an offset to keep it symetric.
fn indicator_angle(normalized: f32) -> f32 {
    0.75 * PI + normalized * 1.5 * PI
}
