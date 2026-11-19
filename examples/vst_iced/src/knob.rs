use std::f32::consts::PI;

use iced::gradient::ColorStop;
use iced::widget::canvas::Gradient;
use iced::widget::canvas::{Cache, LineCap, Path, Stroke, stroke};
use iced::widget::{Action, canvas, container};
use iced::window::Position;
use iced::{Color, Element, Event};
use iced::{Degrees, Fill, Font, Point, Radians, Rectangle, Renderer, Size, Subscription, Theme};
use iced::{event, mouse};

const SENSITIVITY: f32 = 1.0 / 200.0;

/// A basic radial knob
#[derive(Debug)]
pub struct Knob {
    /// The normalized value from [0, 1] for the parameter
    normalized: f32,
    last_pos: Option<Point>,
    dragging: bool,
    cache: Cache,
}

impl Knob {
    pub fn new() -> Self {
        Knob {
            dragging: false,
            last_pos: None,
            normalized: 0.5,
            cache: Cache::default(),
        }
    }

    pub fn update(&mut self, message: KnobMsg) {
        match message {
            KnobMsg::Update { delta, pos } => {
                dbg!(self.normalized);
                self.last_pos = Some(pos);
                self.normalized = (self.normalized + delta).clamp(0.0, 1.0);
                self.cache.clear();
            }
            KnobMsg::StartDrag => {
                dbg!("Start Drag");
                self.dragging = true;
                self.last_pos = None
            }
            KnobMsg::EndDrag => {
                self.dragging = false;
                self.last_pos = None;
            }
        }
    }

    pub fn view(&self) -> Element<'_, KnobMsg> {
        let canvas = canvas(self).width(Fill).height(Fill);
        container(canvas).padding(20).into()
    }
}

pub enum KnobMsg {
    StartDrag,
    EndDrag,
    Update { delta: f32, pos: Point },
}

impl canvas::Program<KnobMsg> for Knob {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: nice_plug_iced::iced::core::mouse::Cursor,
    ) -> Option<Action<KnobMsg>> {
        if let Event::Mouse(inner) = event {
            match inner {
                mouse::Event::ButtonPressed(inner) => {
                    Some(Action::publish(KnobMsg::StartDrag).and_capture())
                }
                mouse::Event::ButtonReleased(_) => {
                    Some(Action::publish(KnobMsg::EndDrag).and_capture())
                }
                mouse::Event::CursorMoved { position } => {
                    if !self.dragging {
                        return None;
                    }

                    let last = self.last_pos.unwrap_or(*position);

                    let diff = last.x - position.x;

                    dbg!(&diff);

                    let delta = diff * SENSITIVITY;

                    Some(
                        Action::publish(KnobMsg::Update {
                            delta,
                            pos: *position,
                        })
                        .and_capture(),
                    )
                }
                _ => None,
            }
        } else {
            None
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            const WIDTH: f32 = 4.0;

            let center = frame.center();

            let top = (frame.height() / 2.0) + center.y;
            let bottom = center.y - (frame.height() / 2.0);

            // TODO: proper padding?
            let radius = 0.9 * (frame.width().min(frame.height()) / 2.0) - WIDTH * 2.0;

            let border = Path::circle(center, radius);

            // TODO: Why is this on the other side?
            //
            // Is this a different coordinate system?
            let indicator_phase = -self.normalized * PI;

            // TODO: Diff with padding as opposed to some
            // fraction
            let indicator_pos = Point::new(
                (indicator_phase.cos() * radius * 1.06) + center.x,
                (indicator_phase.sin() * radius * 1.06) + center.y,
            );

            frame.fill(
                &Path::circle(indicator_pos, 4.0),
                Color::from_rgb8(255, 255, 255),
            );

            frame.stroke(
                &border,
                Stroke {
                    width: WIDTH,
                    style: stroke::Style::Gradient(Gradient::Linear(canvas::gradient::Linear {
                        start: Point::new(center.x, bottom),
                        end: Point::new(center.x, top),
                        stops: [
                            Some(ColorStop {
                                offset: 0.0,
                                color: Color::from_rgb8(212, 167, 255),
                            }),
                            Some(ColorStop {
                                offset: 1.0,
                                color: Color::from_rgb8(50, 38, 55),
                            }),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                        ],
                    })),
                    line_cap: LineCap::Round,
                    ..Stroke::default()
                },
            );
        });

        vec![geometry]
    }
}
