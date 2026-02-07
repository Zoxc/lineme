// Header uses explicit f64 scroll offsets passed from the application state.
use crate::timeline::ticks::nice_interval;
use crate::Message;
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

        // Convert an absolute timestamp (ns) into a screen-space x position.
        let scroll_offset_x_ns = (self.scroll_offset_x / self.zoom_level.max(1e-9)).max(0.0);
        let screen_x = |relative_ns: f64| -> f32 {
            ((relative_ns - scroll_offset_x_ns) * self.zoom_level) as f32
        };

        // Calculate the first visible tick position
        let mut relative_ns = if nice_interval > 0.0 {
            (scroll_offset_x_ns / nice_interval).floor() * nice_interval
        } else {
            0.0
        };

        // Layer heights
        let layer_height = bounds.height / 3.0;

        // Estimate how much horizontal space a label can occupy so we keep
        // drawing ticks until their labels are fully out of view. This avoids
        // truncating ticks whose text would still be visible at the edges.
        let label_padding: f32 = 64.0;

        while relative_ns <= total_ns {
            let x = screen_x(relative_ns);

            // Stop once the tick and its label would be completely off the right
            // edge of the header.
            if x > bounds.width + label_padding {
                break;
            }

            // Skip ticks that are fully left of the visible area (including
            // their label width).
            if x + label_padding < 0.0 {
                relative_ns += nice_interval;
                continue;
            }

            // Draw a full-height vertical line for this tick. Make major (second)
            // ticks darker and slightly wider so they stand out.
            // Decide tick level by exact divisibility of the timestamp.

            // Calculate time components
            let ns_total = relative_ns as u64;
            let seconds = ns_total / 1_000_000_000;
            let ns_remainder = ns_total % 1_000_000_000;
            let ms = ns_remainder / 1_000_000;
            let us_remainder = ns_remainder % 1_000_000;
            // us value (integer microseconds) intentionally unused; keep us_remainder
            // for fractional microsecond display below.

            // Layer 1 (top): Seconds (display as MM:SS)
            // Use slightly smaller top padding and larger font to fit 55px total height.
            let y1 = 4.0;
            // Format seconds as minutes:seconds with leading zeros
            let minutes = seconds / 60;
            let seconds_rem = seconds % 60;
            let s_str = format!("{:02}:{:02}", minutes, seconds_rem);
            frame.fill_text(canvas::Text {
                content: s_str,
                position: Point::new(x + 2.0, y1),
                color: Color::from_rgb(0.2, 0.2, 0.2),
                size: 11.0.into(),
                ..Default::default()
            });

            // Layer 2 (middle): Milliseconds
            let y2 = layer_height + 4.0;
            let ms_str = format!("{:03} ms", ms);
            frame.fill_text(canvas::Text {
                content: ms_str,
                position: Point::new(x + 2.0, y2),
                color: Color::from_rgb(0.3, 0.3, 0.3),
                size: 11.0.into(),
                ..Default::default()
            });

            // Layer 3 (bottom): Microseconds (show two decimal places)
            let y3 = layer_height * 2.0 + 4.0;
            let micro_float = (us_remainder as f64) / 1000.0; // µs with fractional part
            let us_str = format!("{:.2} µs", micro_float);
            frame.fill_text(canvas::Text {
                content: us_str,
                position: Point::new(x + 2.0, y3),
                color: Color::from_rgb(0.4, 0.4, 0.4),
                size: 11.0.into(),
                ..Default::default()
            });

            // Draw full-height tick with styling based on tick significance
            let is_second_tick = ns_total % 1_000_000_000 == 0;
            let is_ms_tick = ns_total % 1_000_000 == 0;
            // Darken header tick lines to increase contrast against the light background.
            let (tick_color, tick_width) = if is_second_tick {
                (Color::from_rgb(0.18, 0.18, 0.18), 1.0)
            } else if is_ms_tick {
                (Color::from_rgb(0.36, 0.36, 0.36), 0.8)
            } else {
                (Color::from_rgb(0.55, 0.55, 0.55), 0.5)
            };
            frame.stroke(
                &canvas::Path::line(Point::new(x, 0.0), Point::new(x, bounds.height)),
                canvas::Stroke::default()
                    .with_color(tick_color)
                    .with_width(tick_width),
            );

            relative_ns += nice_interval;
        }

        // Draw separator lines between layers
        frame.stroke(
            &canvas::Path::line(
                Point::new(0.0, layer_height),
                Point::new(bounds.width, layer_height),
            ),
            canvas::Stroke::default()
                .with_color(Color::from_rgb(0.85, 0.85, 0.85))
                .with_width(0.5),
        );
        frame.stroke(
            &canvas::Path::line(
                Point::new(0.0, layer_height * 2.0),
                Point::new(bounds.width, layer_height * 2.0),
            ),
            canvas::Stroke::default()
                .with_color(Color::from_rgb(0.85, 0.85, 0.85))
                .with_width(0.5),
        );

        vec![frame.into_geometry()]
    }
}
