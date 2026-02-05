use crate::Message;
use iced::mouse;
use iced::widget::canvas::{self, Geometry, Program};
use iced::{keyboard, Color, Point, Rectangle, Renderer, Size, Theme, Vector};

use super::{
    color_from_label, group_total_height, mipmap_levels_for_zoom, visible_event_indices_in,
    ColorMode, ThreadGroup, TimelineEvent,
};
use super::{EVENT_LEFT_PADDING, LANE_HEIGHT};

fn draw_event_rect(
    frame: &mut canvas::Frame,
    x: f32,
    width: f32,
    y: f32,
    color: Color,
    label: &str,
    is_root: bool,
) {
    let rect = Rectangle {
        x,
        y: y + 1.0,
        width: width.max(1.0),
        height: (LANE_HEIGHT - 2.0) as f32,
    };

    frame.fill_rectangle(rect.position(), rect.size(), color);

    let border_color = if is_root {
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
        // Draw the full label and rely on the canvas clip region so overflowing
        // glyphs are visually cropped at the event rectangle boundary.
        frame.with_clip(
            Rectangle {
                x: rect.x + 1.0,
                y: rect.y + 1.0,
                width: rect.width - 2.0,
                height: rect.height - 2.0,
            },
            |frame| {
                frame.fill_text(canvas::Text {
                    content: label.to_string(),
                    position: Point::new(rect.x + 2.0 + EVENT_LEFT_PADDING as f32, rect.y + 2.0),
                    color: if is_root {
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

pub struct EventsProgram<'a> {
    pub thread_groups: &'a [ThreadGroup],
    pub min_ns: u64,
    pub max_ns: u64,
    pub zoom_level: f64,
    pub selected_event: &'a Option<TimelineEvent>,
    pub scroll_offset_x: f64,
    pub scroll_offset_y: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
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
        let zoom_level = self.zoom_level.max(1e-9);
        let scroll_offset_x_px = (self.scroll_offset_x * zoom_level) as f32;
        let content_position = Point::new(
            position.x + scroll_offset_x_px,
            position.y + self.scroll_offset_y as f32,
        );
        let mut y_offset: f64 = 0.0;
        for group in self.thread_groups {
            let lane_total_height = group_total_height(group);

            if (content_position.y as f64) >= y_offset
                && (content_position.y as f64) < (y_offset + lane_total_height as f64)
            {
                let (ns_min, ns_max) = crate::timeline::viewport_ns_range(
                    self.scroll_offset_x,
                    self.viewport_width,
                    zoom_level,
                    self.min_ns,
                );

                for level in mipmap_levels_for_zoom(group, self.zoom_level) {
                    for index in visible_event_indices_in(
                        &level.events,
                        &level.events_by_start,
                        &level.events_by_end,
                        ns_min,
                        ns_max,
                    ) {
                        let event = &level.events[index];
                        if group.is_collapsed && event.depth > 0 {
                            continue;
                        }

                        let width =
                            crate::timeline::duration_to_width(event.duration_ns, self.zoom_level)
                                as f32;
                        if width < 5.0 {
                            continue;
                        }

                        let x = crate::timeline::ns_to_x(event.start_ns, self.min_ns, zoom_level)
                            as f32;
                        let y = y_offset as f32 + event.depth as f32 * (LANE_HEIGHT as f32);
                        let height = (LANE_HEIGHT - 2.0) as f32;

                        let rect = Rectangle {
                            x,
                            y,
                            width: width.max(1.0),
                            height,
                        };

                        if rect.contains(content_position) {
                            return Some(event.clone());
                        }
                    }
                }
            }
            y_offset += lane_total_height as f64 + super::LANE_SPACING as f64;
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

        let viewport_width = if self.viewport_width > 0.0 {
            self.viewport_width
        } else {
            bounds.width as f64
        };
        let viewport_height = if self.viewport_height > 0.0 {
            self.viewport_height
        } else {
            bounds.height as f64
        };

        // Draw vertical tick guide lines matching the header ticks.
        let total_ns = self.max_ns.saturating_sub(self.min_ns) as f64;
        let zoom_level = self.zoom_level.max(1e-9);
        let scroll_offset_x_px = self.scroll_offset_x * zoom_level;
        let ns_min = self.scroll_offset_x.max(0.0) as u64 + self.min_ns;
        let ns_max =
            (self.scroll_offset_x + viewport_width / zoom_level).max(0.0) as u64 + self.min_ns;

        if total_ns > 0.0 {
            // ns per pixel given current zoom: 1 / zoom_level
            let ns_per_pixel = 1.0 / zoom_level;
            let pixel_interval = 100.0;
            let ns_interval = pixel_interval as f64 * ns_per_pixel;
            let nice_interval = crate::timeline::ticks::nice_interval(ns_interval);

            let mut relative_ns = if viewport_width > 0.0 {
                (scroll_offset_x_px / zoom_level / nice_interval).floor() * nice_interval
            } else {
                0.0
            };

            while relative_ns <= total_ns {
                let x_screen = (relative_ns * zoom_level - scroll_offset_x_px) as f32;
                if viewport_width > 0.0 && (x_screen as f64) > viewport_width {
                    break;
                }

                if x_screen < 0.0 {
                    relative_ns += nice_interval;
                    continue;
                }

                // Draw faint vertical line across the events area.
                frame.stroke(
                    &canvas::Path::line(
                        Point::new(x_screen, 0.0),
                        Point::new(x_screen, bounds.height),
                    ),
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.5, 0.5, 0.5, 0.3))
                        .with_width(1.0),
                );

                relative_ns += nice_interval;
            }
        }

        let mut y_offset: f64 = 0.0;
        let y_min = self.scroll_offset_y;
        let y_max = self.scroll_offset_y + viewport_height;

        for group in self.thread_groups {
            let lane_total_height = group_total_height(group);

            // Skip drawing if thread is completely outside vertical viewport
            if self.viewport_height > 0.0
                && ((y_offset + lane_total_height as f64) < y_min || y_offset > y_max)
            {
                y_offset += lane_total_height as f64 + super::LANE_SPACING as f64;
                continue;
            }

            let row_y = y_offset as f32 - self.scroll_offset_y as f32;
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, row_y), Point::new(bounds.width, row_y)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.9, 0.9, 0.9))
                    .with_width(1.0),
            );

            for level in mipmap_levels_for_zoom(group, zoom_level) {
                for index in visible_event_indices_in(
                    &level.events,
                    &level.events_by_start,
                    &level.events_by_end,
                    ns_min,
                    ns_max,
                ) {
                    let event = &level.events[index];
                    if group.is_collapsed && event.depth > 0 {
                        continue;
                    }

                    let width =
                        crate::timeline::duration_to_width(event.duration_ns, zoom_level) as f32;
                    if width < 1.0 {
                        continue;
                    }

                    let x =
                        crate::timeline::ns_to_x(event.start_ns, self.min_ns, zoom_level) as f32;

                    // Skip drawing if event is completely outside horizontal viewport
                    let x_screen = x - scroll_offset_x_px as f32;
                    if viewport_width > 0.0
                        && ((x_screen + width) < 0.0 || (x_screen as f64) > viewport_width)
                    {
                        continue;
                    }

                    let depth = event.depth as usize;
                    let color = if event.is_thread_root {
                        event.color
                    } else {
                        match self.color_mode {
                            // When coloring by kind we already stored a kind-based color
                            // on the TimelineEvent during data loading.
                            ColorMode::Kind => event.color,
                            ColorMode::Event => color_from_label(&event.label),
                        }
                    };
                    let label = &event.label;
                    let is_thread_root = event.is_thread_root;

                    let x_screen = x - scroll_offset_x_px as f32;
                    let y_screen = y_offset as f32 - self.scroll_offset_y as f32
                        + depth as f32 * (LANE_HEIGHT as f32);
                    draw_event_rect(
                        &mut frame,
                        x_screen,
                        width,
                        y_screen,
                        color,
                        label,
                        is_thread_root,
                    );
                }
            }

            if let Some(hovered) = &state.hovered_event {
                if super::group_contains_thread(group, hovered.thread_id) {
                    if !group.is_collapsed || hovered.depth == 0 {
                        let x = crate::timeline::ns_to_x(hovered.start_ns, self.min_ns, zoom_level)
                            as f32;
                        let width =
                            crate::timeline::duration_to_width(hovered.duration_ns, zoom_level)
                                as f32;
                        let x_screen = x - scroll_offset_x_px as f32;
                        let y = y_offset as f32 - self.scroll_offset_y as f32
                            + hovered.depth as f32 * (LANE_HEIGHT as f32);

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x_screen, y + 1.0),
                                Size::new(width.max(1.0), (LANE_HEIGHT - 2.0) as f32),
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
                        let x = crate::timeline::ns_to_x(selected.start_ns, self.min_ns, zoom_level)
                            as f32;
                        let width =
                            crate::timeline::duration_to_width(selected.duration_ns, zoom_level)
                                as f32;
                        let x_screen = x - scroll_offset_x_px as f32;
                        let y = y_offset as f32 - self.scroll_offset_y as f32
                            + selected.depth as f32 * (LANE_HEIGHT as f32);

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x_screen, y + 1.0),
                                Size::new(width.max(1.0), (LANE_HEIGHT - 2.0) as f32),
                            ),
                            canvas::Stroke::default()
                                .with_color(Color::from_rgb(0.0, 0.4, 0.8))
                                .with_width(2.0),
                        );
                    }
                }
            }

            y_offset += lane_total_height as f64 + super::LANE_SPACING as f64;
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
                    if !state.dragging && delta.x.hypot(delta.y) > super::DRAG_THRESHOLD as f32 {
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
                                    return Some(
                                        canvas::Action::publish(Message::TimelinePanned {
                                            delta: Vector::new(scroll_amount, 0.0),
                                        })
                                        .and_capture(),
                                    );
                                }
                            }
                        }
                    } else if state.modifiers.control() {
                        match delta {
                            iced::mouse::ScrollDelta::Lines { x: _, y }
                            | iced::mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    let scroll_amount = (*y as f32) * 30.0;
                                    return Some(
                                        canvas::Action::publish(Message::TimelinePanned {
                                            delta: Vector::new(0.0, scroll_amount),
                                        })
                                        .and_capture(),
                                    );
                                }
                            }
                        }
                    } else {
                        match delta {
                            iced::mouse::ScrollDelta::Lines { x: _, y }
                            | iced::mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    let viewport_width = self.viewport_width.max(0.0);
                                    let cursor_x = (position.x as f64).clamp(0.0, viewport_width);
                                    return Some(
                                        canvas::Action::publish(Message::TimelineZoomed {
                                            delta: *y,
                                            x: cursor_x as f32,
                                        })
                                        .and_capture(),
                                    );
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
