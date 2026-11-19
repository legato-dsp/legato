use iced::font::Weight::Black;
use iced::gradient::ColorStop;
use iced::mouse;
use iced::widget::canvas::Gradient;
use iced::widget::canvas::{Cache, LineCap, Path, Stroke, stroke};
use iced::widget::{canvas, container};
use iced::{Color, Element};
use iced::{Degrees, Fill, Font, Point, Radians, Rectangle, Renderer, Size, Subscription, Theme};

/// A basic radial knob
#[derive(Debug)]
pub struct Knob {
    /// The normalized value from [0, 1] for the parameter
    normalized: f32,
    cache: Cache,
}

impl Knob {
    pub fn new() -> Self {
        Knob {
            normalized: 0.5,
            cache: Cache::default(),
        }
    }
    pub fn update(&mut self, message: KnobMsg) {
        match message {
            KnobMsg::Update { normalized } => self.normalized = normalized,
        }
    }
    pub fn view(&self) -> Element<'_, KnobMsg> {
        let canvas = canvas(self).width(Fill).height(Fill);
        container(canvas).padding(20).into()
    }
}

pub enum KnobMsg {
    Update { normalized: f32 },
}

impl canvas::Program<KnobMsg> for Knob {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            let palette = theme.palette();

            const WIDTH: f32 = 4.0;

            let center = frame.center();

            let top = (frame.height() / 2.0) + center.y;
            let bottom = center.y - (frame.height() / 2.0);

            let radius = (frame.width().min(frame.height()) / 2.0) - WIDTH * 2.0;

            let border = Path::circle(center, radius);
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
