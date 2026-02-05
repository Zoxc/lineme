use crate::Message;
use iced::mouse;
use iced::widget::canvas::{self, Geometry, Program};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector, keyboard};

use super::{ColorMode, ThreadGroup, TimelineEvent, color_from_label, visible_event_indices};
use super::{EVENT_LEFT_PADDING, LANE_HEIGHT};

pub struct EventsProgram<'a> {
    pub thread_groups: &'a [ThreadGroup],
    pub min_ns: u64,
    pub max_ns: u64,
    pub zoom_level: f32,
    pub selected_event: &'a Option<TimelineEvent>,
    pub scroll_offset: Vector,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub color_mode: ColorMode,
}

#[derive(Default)]
pub struct EventsState {
    pub modifiers: keyboard::Modifiers,
    pub hovered_event: Option<TimelineEvent>,
    pub last_click: Option<(TimelineEvent, std::time::Instant)>,
    pub press_position: Option<Point>,
    pub pressed_event: Option<TimelineEvent>,
    pub dragging: bool,
}

impl<'a> EventsProgram<'a> {
    fn find_event_at(&self, position: Point) -> Option<TimelineEvent> {
        let mut y_offset = 0.0;
        for group in self.thread_groups {
            let lane_total_height = if group.is_collapsed {
                LANE_HEIGHT
            } else {
                (group.max_depth + 1) as f32 * LANE_HEIGHT
            };

            if position.y >= y_offset && position.y < y_offset + lane_total_height {
                let ns_min = (self.scroll_offset.x as f64 / self.zoom_level as f64).max(0.0) as u64
                    + self.min_ns;
                let ns_max = ((self.scroll_offset.x + self.viewport_width) as f64
                    / self.zoom_level as f64)
                    .max(0.0) as u64
                    + self.min_ns;

                for index in visible_event_indices(group, ns_min, ns_max) {
                    let event = &group.events[index];
                    if group.is_collapsed && event.depth > 0 {
                        continue;
                    }

                    let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                    if width < 5.0 {
                        continue;
                    }

                    let x = (event.start_ns.saturating_sub(self.min_ns) as f64
                        * self.zoom_level as f64) as f32;
                    let y = y_offset + event.depth as f32 * LANE_HEIGHT;
                    let height = LANE_HEIGHT - 2.0;

                    let rect = Rectangle {
                        x,
                        y,
                        width: width.max(1.0),
                        height,
                    };

                    if rect.contains(position) {
                        return Some(event.clone());
                    }
                }
            }
            y_offset += lane_total_height + super::LANE_SPACING;
        }
        None
    }
}

impl<'a> Program<Message> for EventsProgram<'a> {
    type State = EventsState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.thread_groups.is_empty() {
            return vec![frame.into_geometry()];
        }

        // Draw vertical tick guide lines matching the header ticks.
        let total_ns = self.max_ns.saturating_sub(self.min_ns) as f64;
        let x_min = self.scroll_offset.x;
        let x_max = self.scroll_offset.x + self.viewport_width;
        let ns_min = (x_min as f64 / self.zoom_level as f64).max(0.0) as u64 + self.min_ns;
        let ns_max = (x_max as f64 / self.zoom_level as f64).max(0.0) as u64 + self.min_ns;

        if total_ns > 0.0 {
            // ns per pixel given current zoom: 1 / zoom_level
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

            let mut relative_ns = if self.viewport_width > 0.0 {
                (x_min as f64 / self.zoom_level as f64 / nice_interval).floor() * nice_interval
            } else {
                0.0
            };

            while relative_ns <= total_ns {
                let x = (relative_ns * self.zoom_level as f64) as f32;
                if self.viewport_width > 0.0 && x > x_max {
                    break;
                }

                // Draw faint vertical line across the events area.
                frame.stroke(
                    &canvas::Path::line(Point::new(x, 0.0), Point::new(x, bounds.height)),
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.5, 0.5, 0.5, 0.3))
                        .with_width(1.0),
                );

                relative_ns += nice_interval;
            }
        }

        let mut y_offset = 0.0;
        let y_min = self.scroll_offset.y;
        let y_max = self.scroll_offset.y + self.viewport_height;

        for group in self.thread_groups {
            let lane_total_height = if group.is_collapsed {
                LANE_HEIGHT
            } else {
                (group.max_depth + 1) as f32 * LANE_HEIGHT
            };

            // Skip drawing if thread is completely outside vertical viewport
            if self.viewport_height > 0.0
                && (y_offset + lane_total_height < y_min || y_offset > y_max)
            {
                y_offset += lane_total_height + super::LANE_SPACING;
                continue;
            }

            frame.stroke(
                &canvas::Path::line(
                    Point::new(0.0, y_offset),
                    Point::new(bounds.width, y_offset),
                ),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.9, 0.9, 0.9))
                    .with_width(1.0),
            );

            let mut last_rects: Vec<Option<(f32, f32, Color, String, bool)>> =
                vec![None; (group.max_depth + 1) as usize];

            for index in visible_event_indices(group, ns_min, ns_max) {
                let event = &group.events[index];
                if group.is_collapsed && event.depth > 0 {
                    continue;
                }

                let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                if width < 5.0 {
                    continue;
                }

                let x = (event.start_ns.saturating_sub(self.min_ns) as f64 * self.zoom_level as f64)
                    as f32;

                // Skip drawing if event is completely outside horizontal viewport
                if self.viewport_width > 0.0 && (x + width < x_min || x > x_max) {
                    continue;
                }

                let depth = event.depth as usize;
                let color = if event.is_thread_root {
                    event.color
                } else {
                    match self.color_mode {
                        ColorMode::Kind => color_from_label(&event.event_kind),
                        ColorMode::Event => color_from_label(&event.label),
                    }
                };
                let label = &event.label;
                let is_thread_root = event.is_thread_root;

                if let Some((cur_x, cur_w, cur_color, cur_label, cur_is_root)) =
                    &mut last_rects[depth]
                {
                    let end_x = *cur_x + *cur_w;
                    if !is_thread_root
                        && color == *cur_color
                        && x <= end_x + 0.5
                        && label == cur_label
                    {
                        let new_end = (x + width).max(end_x);
                        *cur_w = new_end - *cur_x;
                        continue;
                    } else {
                        let y = y_offset + depth as f32 * LANE_HEIGHT;
                        let rect = Rectangle {
                            x: *cur_x,
                            y: y + 1.0,
                            width: cur_w.max(1.0),
                            height: LANE_HEIGHT - 2.0,
                        };

                        frame.fill_rectangle(rect.position(), rect.size(), *cur_color);

                        let border_color = if *cur_is_root {
                            Color::from_rgba(0.0, 0.0, 0.0, 0.35)
                        } else {
                            Color::from_rgba(0.0, 0.0, 0.0, 0.2)
                        };

                        frame.stroke(
                            &canvas::Path::rectangle(rect.position(), rect.size()),
                            canvas::Stroke::default()
                                .with_color(border_color)
                                .with_width(1.0),
                        );

                        if rect.width > 20.0 {
                            let mut truncated_label = cur_label.clone();
                            let avail_chars =
                                ((rect.width - 4.0 - EVENT_LEFT_PADDING).max(0.0) / 6.0) as usize;
                            if truncated_label.len() > avail_chars {
                                truncated_label.truncate(avail_chars);
                            }
                            frame.with_clip(
                                Rectangle {
                                    x: rect.x + 1.0,
                                    y: rect.y + 1.0,
                                    width: rect.width - 2.0,
                                    height: rect.height - 2.0,
                                },
                                |frame| {
                                    frame.fill_text(canvas::Text {
                                        content: truncated_label,
                                        position: Point::new(
                                            rect.x + 2.0 + EVENT_LEFT_PADDING,
                                            rect.y + 2.0,
                                        ),
                                        color: if *cur_is_root {
                                            Color::from_rgb(0.35, 0.35, 0.35)
                                        } else {
                                            Color::from_rgb(0.2, 0.2, 0.2)
                                        },
                                        size: 12.0.into(),
                                        ..Default::default()
                                    });
                                },
                            );
                        }
                    }
                }
                last_rects[depth] = Some((x, width, color, label.clone(), is_thread_root));
            }

            for (depth, rect) in last_rects.into_iter().enumerate() {
                if let Some((cur_x, cur_w, cur_color, cur_label, cur_is_root)) = rect {
                    let y = y_offset + depth as f32 * LANE_HEIGHT;
                    let rect = Rectangle {
                        x: cur_x,
                        y: y + 1.0,
                        width: cur_w.max(1.0),
                        height: LANE_HEIGHT - 2.0,
                    };

                    frame.fill_rectangle(rect.position(), rect.size(), cur_color);

                    let border_color = if cur_is_root {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.35)
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.2)
                    };

                    frame.stroke(
                        &canvas::Path::rectangle(rect.position(), rect.size()),
                        canvas::Stroke::default()
                            .with_color(border_color)
                            .with_width(1.0),
                    );

                    if rect.width > 20.0 {
                        let mut truncated_label = cur_label;
                        let avail_chars =
                            ((rect.width - 4.0 - EVENT_LEFT_PADDING).max(0.0) / 6.0) as usize;
                        if truncated_label.len() > avail_chars {
                            truncated_label.truncate(avail_chars);
                        }
                        frame.with_clip(
                            Rectangle {
                                x: rect.x + 1.0,
                                y: rect.y + 1.0,
                                width: rect.width - 2.0,
                                height: rect.height - 2.0,
                            },
                            |frame| {
                                frame.fill_text(canvas::Text {
                                    content: truncated_label,
                                    position: Point::new(
                                        rect.x + 2.0 + EVENT_LEFT_PADDING,
                                        rect.y + 2.0,
                                    ),
                                    color: if cur_is_root {
                                        Color::from_rgb(0.35, 0.35, 0.35)
                                    } else {
                                        Color::from_rgb(0.2, 0.2, 0.2)
                                    },
                                    size: 12.0.into(),
                                    ..Default::default()
                                });
                            },
                        );
                    }
                }
            }

            if let Some(hovered) = &state.hovered_event {
                if super::group_contains_thread(group, hovered.thread_id) {
                    if !group.is_collapsed || hovered.depth == 0 {
                        let x = (hovered.start_ns.saturating_sub(self.min_ns) as f64
                            * self.zoom_level as f64) as f32;
                        let width = (hovered.duration_ns as f64 * self.zoom_level as f64) as f32;
                        let y = y_offset + hovered.depth as f32 * LANE_HEIGHT;

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x, y + 1.0),
                                Size::new(width.max(1.0), LANE_HEIGHT - 2.0),
                            ),
                            canvas::Stroke::default()
                                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))
                                .with_width(1.0),
                        );
                    }
                }
            }

            if let Some(selected) = self.selected_event {
                if super::group_contains_thread(group, selected.thread_id) {
                    if !group.is_collapsed || selected.depth == 0 {
                        let x = (selected.start_ns.saturating_sub(self.min_ns) as f64
                            * self.zoom_level as f64) as f32;
                        let width = (selected.duration_ns as f64 * self.zoom_level as f64) as f32;
                        let y = y_offset + selected.depth as f32 * LANE_HEIGHT;

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x, y + 1.0),
                                Size::new(width.max(1.0), LANE_HEIGHT - 2.0),
                            ),
                            canvas::Stroke::default()
                                .with_color(Color::from_rgb(0.0, 0.4, 0.8))
                                .with_width(2.0),
                        );
                    }
                }
            }

            y_offset += lane_total_height + super::LANE_SPACING;
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if let (
                    Some(press_position),
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }),
                ) = (state.press_position, event)
                {
                    let delta = *position - press_position;
                    if !state.dragging && delta.x.hypot(delta.y) > super::DRAG_THRESHOLD {
                        state.dragging = true;
                    }
                }
                let new_hovered = cursor
                    .position_in(bounds)
                    .and_then(|p| self.find_event_at(p));

                if new_hovered != state.hovered_event {
                    state.hovered_event = new_hovered;
                    return Some(canvas::Action::publish(Message::EventHovered(
                        state.hovered_event.clone(),
                    )));
                }
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    state.press_position = cursor.position();
                    state.pressed_event = self.find_event_at(position);
                    state.dragging = false;
                }
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if !state.dragging {
                    if let (Some(pressed_event), Some(position)) =
                        (state.pressed_event.clone(), cursor.position_in(bounds))
                    {
                        if let Some(release_event) = self.find_event_at(position) {
                            let is_same_event = pressed_event.start_ns == release_event.start_ns
                                && pressed_event.duration_ns == release_event.duration_ns
                                && pressed_event.thread_id == release_event.thread_id;
                            if is_same_event {
                                let now = std::time::Instant::now();
                                if let Some((prev_event, prev_time)) = &state.last_click {
                                    let is_double = prev_event.start_ns == release_event.start_ns
                                        && prev_event.duration_ns == release_event.duration_ns
                                        && prev_event.thread_id == release_event.thread_id
                                        && now.duration_since(*prev_time)
                                            <= std::time::Duration::from_millis(400);
                                    if is_double {
                                        state.last_click = None;
                                        state.press_position = None;
                                        state.pressed_event = None;
                                        state.dragging = false;
                                        return Some(canvas::Action::publish(
                                            Message::EventDoubleClicked(release_event),
                                        ));
                                    }
                                }

                                state.last_click = Some((release_event.clone(), now));
                                state.press_position = None;
                                state.pressed_event = None;
                                state.dragging = false;
                                return Some(canvas::Action::publish(Message::EventSelected(
                                    release_event,
                                )));
                            }
                        }
                    }
                }
                state.press_position = None;
                state.pressed_event = None;
                state.dragging = false;
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    // Shift + wheel: pan horizontally
                    if state.modifiers.shift() {
                        match delta {
                            iced::mouse::ScrollDelta::Lines { x: _, y }
                            | iced::mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    // Map wheel "lines" to pixels for a comfortable pan speed
                                    let scroll_amount = (*y as f32) * 30.0;
                                    return Some(canvas::Action::publish(
                                        Message::TimelinePanned {
                                            delta: Vector::new(scroll_amount, 0.0),
                                        },
                                    ));
                                }
                            }
                        }
                    // Control (or other) keys: default behavior handled elsewhere — we only
                    // intercept wheel when control is NOT held to provide zoom by wheel.
                    } else if !state.modifiers.control() {
                        match delta {
                            iced::mouse::ScrollDelta::Lines { x: _, y }
                            | iced::mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    let viewport_width = self.viewport_width.max(0.0);
                                    let cursor_x = (position.x - self.scroll_offset.x)
                                        .clamp(0.0, viewport_width);
                                    return Some(canvas::Action::publish(
                                        Message::TimelineZoomed {
                                            delta: *y,
                                            x: cursor_x,
                                        },
                                    ));
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
