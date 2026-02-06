// Header uses explicit f64 scroll offsets passed from the application state.
use crate::Message;
use crate::timeline::ticks::{format_time_label, nice_interval};
use iced::mouse;
use iced::widget::canvas::{self, Geometry, Program};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

pub(crate) struct HeaderProgram {
    pub(crate) min_ns: u64,
    pub(crate) max_ns: u64,
    pub(crate) zoom_level: f64,
    pub(crate) scroll_offset_x: f64,
}

impl Program<Message> for HeaderProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(bounds.width, bounds.height),
            Color::from_rgb(0.95, 0.95, 0.95),
        );

        let total_ns = crate::timeline::total_ns(self.min_ns, self.max_ns) as f64;
        if total_ns <= 0.0 {
            return vec![frame.into_geometry()];
        }

        let ns_per_pixel = 1.0 / self.zoom_level;
        let pixel_interval = 100.0;
        let ns_interval = pixel_interval * ns_per_pixel;
        let nice_interval = nice_interval(ns_interval);

        let mut relative_ns = 0.0;
        while relative_ns <= total_ns {
            let x = (relative_ns * self.zoom_level) as f32 - self.scroll_offset_x as f32;

            if x >= 0.0 && x <= bounds.width {
                frame.stroke(
                    &canvas::Path::line(
                        Point::new(x, bounds.height - 5.0),
                        Point::new(x, bounds.height),
                    ),
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(0.5, 0.5, 0.5))
                        .with_width(1.0),
                );

                let time_str = format_time_label(relative_ns, nice_interval);

                frame.fill_text(canvas::Text {
                    content: time_str,
                    position: Point::new(x + 2.0, 5.0),
                    color: Color::from_rgb(0.3, 0.3, 0.3),
                    size: 12.0.into(),
                    ..Default::default()
                });
            }
            relative_ns += nice_interval;
        }

        vec![frame.into_geometry()]
    }
}
