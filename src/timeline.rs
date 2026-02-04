use crate::Message;
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{layout, renderer, Clipboard, Layout, Shell};
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::Action;
use iced::widget::canvas::{self, Canvas, Geometry, Program};
use iced::widget::{column, container, scrollable, text};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

pub const LABEL_WIDTH: f32 = 150.0;
pub const HEADER_HEIGHT: f32 = 30.0;
pub const LANE_HEIGHT: f32 = 20.0;
pub const LANE_SPACING: f32 = 5.0;

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub label: String,
    pub start_ns: u64,
    pub duration_ns: u64,
    pub depth: u32,
    pub thread_id: u64,
    pub color: Color,
}

#[derive(Debug, Clone)]
pub struct ThreadData {
    pub thread_id: u64,
    pub events: Vec<TimelineEvent>,
    pub max_depth: u32,
    pub is_collapsed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TimelineData {
    pub threads: Vec<ThreadData>,
    pub min_ns: u64,
    pub max_ns: u64,
}

pub fn color_from_label(label: &str) -> Color {
    let mut hash = 0u64;
    for c in label.chars() {
        hash = hash.wrapping_add(c as u64);
        hash = hash.wrapping_mul(0x517cc1b727220a95);
    }

    let r = ((hash >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hash >> 8) & 0xFF) as f32 / 255.0;
    let b = (hash & 0xFF) as f32 / 255.0;

    Color::from_rgb(0.3 + r * 0.4, 0.3 + g * 0.4, 0.3 + b * 0.4)
}

pub fn timeline_id() -> iced::widget::Id {
    iced::widget::Id::new("timeline_scrollable")
}

pub fn view<'a>(
    timeline_data: &'a TimelineData,
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
    scroll_offset: Vector,
    modifiers: keyboard::Modifiers,
) -> Element<'a, Message> {
    let total_ns = timeline_data.max_ns - timeline_data.min_ns;
    if total_ns == 0 {
        return container(text("No events to display"))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
    }

    let mut total_height = HEADER_HEIGHT;
    for thread in &timeline_data.threads {
        let lane_total_height = if thread.is_collapsed {
            LANE_HEIGHT
        } else {
            (thread.max_depth + 1) as f32 * LANE_HEIGHT
        };
        total_height += lane_total_height + LANE_SPACING;
    }

    let canvas_width = total_ns as f32 * zoom_level + LABEL_WIDTH;

    let timeline_canvas = Canvas::new(TimelineProgram {
        threads: &timeline_data.threads,
        min_ns: timeline_data.min_ns,
        zoom_level,
        selected_event,
        scroll_offset,
    })
    .width(Length::Fixed(canvas_width))
    .height(Length::Fixed(total_height));

    let main_view = scrollable(WheelCatcher::new(timeline_canvas, modifiers))
        .id(timeline_id())
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .on_scroll(|viewport| Message::TimelineScroll {
            offset: Vector::new(viewport.absolute_offset().x, viewport.absolute_offset().y),
        });

    let details_panel = if let Some(event) = selected_event {
        container(
            column![
                text(format!("Event: {}", event.label)).size(20),
                text(format!("Thread: {}", event.thread_id)),
                text(format!("Start: {} ns", event.start_ns)),
                text(format!("Duration: {} ns", event.duration_ns)),
            ]
            .spacing(5)
            .padding(10),
        )
        .width(Length::Fill)
        .height(Length::Fixed(120.0))
    } else {
        container(text("Select an event to see details"))
            .width(Length::Fill)
            .height(Length::Fixed(120.0))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    };

    column![main_view, details_panel].into()
}

struct TimelineProgram<'a> {
    threads: &'a [ThreadData],
    min_ns: u64,
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
    scroll_offset: Vector,
}

#[derive(Default)]
struct TimelineState {
    modifiers: keyboard::Modifiers,
}

impl<'a> Program<Message> for TimelineProgram<'a> {
    type State = TimelineState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.threads.is_empty() {
            return vec![frame.into_geometry()];
        }

        let total_ns = self
            .threads
            .first()
            .map(|_| (bounds.width - LABEL_WIDTH) / self.zoom_level)
            .unwrap_or(0.0);

        let mut y_offset = HEADER_HEIGHT;
        for thread in self.threads {
            let lane_total_height = if thread.is_collapsed {
                LANE_HEIGHT
            } else {
                (thread.max_depth + 1) as f32 * LANE_HEIGHT
            };

            frame.stroke(
                &canvas::Path::line(
                    Point::new(self.scroll_offset.x, y_offset),
                    Point::new(bounds.width, y_offset),
                ),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.2, 0.2, 0.2))
                    .with_width(1.0),
            );

            let mut last_rects: Vec<Option<(f32, f32, Color)>> =
                vec![None; (thread.max_depth + 1) as usize];

            for event in &thread.events {
                if thread.is_collapsed && event.depth > 0 {
                    continue;
                }

                let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                if width < 5.0 {
                    continue;
                }

                let x = (event.start_ns.saturating_sub(self.min_ns) as f64 * self.zoom_level as f64)
                    as f32
                    + LABEL_WIDTH;
                let depth = event.depth as usize;
                let color = event.color;

                if let Some((cur_x, cur_w, cur_color)) = last_rects[depth] {
                    let end_x = cur_x + cur_w;
                    if color == cur_color && x <= end_x + 0.5 {
                        let new_end = (x + width).max(end_x);
                        last_rects[depth] = Some((cur_x, new_end - cur_x, cur_color));
                        continue;
                    } else {
                        let y = y_offset + depth as f32 * LANE_HEIGHT;
                        let rect = Rectangle {
                            x: cur_x,
                            y: y + 1.0,
                            width: cur_w.max(1.0),
                            height: LANE_HEIGHT - 2.0,
                        };

                        frame.fill_rectangle(rect.position(), rect.size(), cur_color);

                        frame.stroke(
                            &canvas::Path::rectangle(rect.position(), rect.size()),
                            canvas::Stroke::default()
                                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))
                                .with_width(1.0),
                        );
                    }
                }
                last_rects[depth] = Some((x, width, color));
            }

            for (depth, rect) in last_rects.into_iter().enumerate() {
                if let Some((cur_x, cur_w, cur_color)) = rect {
                    let y = y_offset + depth as f32 * LANE_HEIGHT;
                    let rect = Rectangle {
                        x: cur_x,
                        y: y + 1.0,
                        width: cur_w.max(1.0),
                        height: LANE_HEIGHT - 2.0,
                    };

                    frame.fill_rectangle(rect.position(), rect.size(), cur_color);

                    frame.stroke(
                        &canvas::Path::rectangle(rect.position(), rect.size()),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))
                            .with_width(1.0),
                    );
                }
            }

            if let Some(selected) = self.selected_event {
                if selected.thread_id == thread.thread_id {
                    if !thread.is_collapsed || selected.depth == 0 {
                        let x = (selected.start_ns.saturating_sub(self.min_ns) as f64
                            * self.zoom_level as f64) as f32
                            + LABEL_WIDTH;
                        let width = (selected.duration_ns as f64 * self.zoom_level as f64) as f32;
                        let y = y_offset + selected.depth as f32 * LANE_HEIGHT;

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x, y + 1.0),
                                Size::new(width.max(1.0), LANE_HEIGHT - 2.0),
                            ),
                            canvas::Stroke::default()
                                .with_color(Color::WHITE)
                                .with_width(2.0),
                        );
                    }
                }
            }

            y_offset += lane_total_height + LANE_SPACING;
        }

        frame.fill_rectangle(
            Point::new(self.scroll_offset.x, self.scroll_offset.y),
            Size::new(bounds.width, HEADER_HEIGHT),
            Color::from_rgb(0.15, 0.15, 0.15),
        );

        if total_ns > 0.0 {
            let canvas_width = bounds.width;
            let ns_per_pixel = 1.0 / self.zoom_level as f64;

            let pixel_interval = 100.0;
            let ns_interval = pixel_interval as f64 * ns_per_pixel;

            let log10 = ns_interval.log10().floor();
            let base = 10.0f64.powf(log10);
            let nice_interval = if ns_interval / base < 2.0 {
                base * 2.0
            } else if ns_interval / base < 5.0 {
                base * 5.0
            } else {
                base * 10.0
            };

            let first_marker = (self.min_ns as f64 / nice_interval).floor() * nice_interval;
            let mut current_marker = first_marker;

            while ((current_marker - self.min_ns as f64) * self.zoom_level as f64)
                < canvas_width as f64
            {
                let x = ((current_marker - self.min_ns as f64) * self.zoom_level as f64) as f32
                    + LABEL_WIDTH;

                if x >= LABEL_WIDTH + self.scroll_offset.x {
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(x, self.scroll_offset.y + HEADER_HEIGHT),
                            Point::new(x, bounds.height),
                        ),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgb(0.18, 0.18, 0.18))
                            .with_width(1.0),
                    );

                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(x, self.scroll_offset.y + HEADER_HEIGHT - 5.0),
                            Point::new(x, self.scroll_offset.y + HEADER_HEIGHT),
                        ),
                        canvas::Stroke::default()
                            .with_color(Color::WHITE)
                            .with_width(1.0),
                    );

                    let time_str = if nice_interval >= 1_000_000_000.0 {
                        format!("{:.2} s", current_marker / 1_000_000_000.0)
                    } else if nice_interval >= 1_000_000.0 {
                        format!("{:.2} ms", current_marker / 1_000_000.0)
                    } else if nice_interval >= 1_000.0 {
                        format!("{:.2} µs", current_marker / 1_000.0)
                    } else {
                        format!("{} ns", current_marker)
                    };

                    frame.fill_text(canvas::Text {
                        content: time_str,
                        position: Point::new(x + 2.0, self.scroll_offset.y + 5.0),
                        color: Color::WHITE,
                        size: 10.0.into(),
                        ..Default::default()
                    });
                }
                current_marker += nice_interval;
            }
        }

        frame.fill_rectangle(
            Point::new(self.scroll_offset.x, self.scroll_offset.y + HEADER_HEIGHT),
            Size::new(LABEL_WIDTH, bounds.height - HEADER_HEIGHT),
            Color::from_rgb(0.1, 0.1, 0.1),
        );

        y_offset = HEADER_HEIGHT;
        for thread in self.threads {
            let lane_total_height = if thread.is_collapsed {
                LANE_HEIGHT
            } else {
                (thread.max_depth + 1) as f32 * LANE_HEIGHT
            };

            let label_text = if thread.is_collapsed {
                format!("▶ Thread {}", thread.thread_id)
            } else {
                format!("▼ Thread {}", thread.thread_id)
            };

            frame.fill_text(canvas::Text {
                content: label_text,
                position: Point::new(self.scroll_offset.x + 5.0, y_offset + 5.0),
                color: Color::WHITE,
                size: 12.0.into(),
                ..Default::default()
            });

            y_offset += lane_total_height + LANE_SPACING;
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if position.x < LABEL_WIDTH + self.scroll_offset.x {
                        let mut y_offset = HEADER_HEIGHT;
                        for thread in self.threads {
                            let lane_total_height = if thread.is_collapsed {
                                LANE_HEIGHT
                            } else {
                                (thread.max_depth + 1) as f32 * LANE_HEIGHT
                            };

                            if position.y >= y_offset && position.y < y_offset + lane_total_height {
                                return Some(Action::publish(Message::ToggleThreadCollapse(
                                    thread.thread_id,
                                )));
                            }
                            y_offset += lane_total_height + LANE_SPACING;
                        }
                        return None;
                    }

                    let mut y_offset = HEADER_HEIGHT;
                    for thread in self.threads {
                        let lane_total_height = if thread.is_collapsed {
                            LANE_HEIGHT
                        } else {
                            (thread.max_depth + 1) as f32 * LANE_HEIGHT
                        };

                        if position.y >= y_offset && position.y < y_offset + lane_total_height {
                            for event in &thread.events {
                                if thread.is_collapsed && event.depth > 0 {
                                    continue;
                                }

                                let width =
                                    (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                                if width < 5.0 {
                                    continue;
                                }

                                let x = (event.start_ns.saturating_sub(self.min_ns) as f64
                                    * self.zoom_level as f64)
                                    as f32
                                    + LABEL_WIDTH;
                                let y = y_offset + event.depth as f32 * LANE_HEIGHT;
                                let height = LANE_HEIGHT - 2.0;

                                let rect = Rectangle {
                                    x,
                                    y,
                                    width: width.max(1.0),
                                    height,
                                };

                                if rect.contains(position) {
                                    return Some(Action::publish(Message::EventSelected(
                                        event.clone(),
                                    )));
                                }
                            }
                        }
                        y_offset += lane_total_height + LANE_SPACING;
                    }
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if !state.modifiers.control() {
                        match delta {
                            mouse::ScrollDelta::Lines { x: _, y }
                            | mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    return Some(Action::publish(Message::TimelineZoomed {
                                        delta: *y,
                                        x: position.x - self.scroll_offset.x,
                                    }));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        None
    }
}

pub struct WheelCatcher<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    modifiers: keyboard::Modifiers,
}

impl<'a, Message, Theme, Renderer> WheelCatcher<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        modifiers: keyboard::Modifiers,
    ) -> Self {
        Self {
            content: content.into(),
            modifiers,
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for WheelCatcher<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            _viewport,
        );

        if let Event::Mouse(mouse::Event::WheelScrolled { .. }) = event {
            if !self.modifiers.control() {
                shell.capture_event();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

impl<'a, Message, Theme, Renderer> From<WheelCatcher<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(catcher: WheelCatcher<'a, Message, Theme, Renderer>) -> Self {
        Self::new(catcher)
    }
}
