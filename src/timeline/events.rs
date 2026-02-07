use crate::Message;
use iced::mouse;
use iced::widget::canvas::{self, Geometry, Program};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector, keyboard};

use super::{EVENT_LEFT_PADDING, LANE_HEIGHT};
use super::{
    EventId, ThreadGroup, TimelineEvent, color_from_label, group_total_height,
    visible_event_indices_in, visible_shadows_in,
};
use crate::data::{ColorMode, display_depth};

// Small helper struct to avoid too_many_arguments lint on the drawing helper.
struct DrawEventRectArgs<'a> {
    frame: &'a mut canvas::Frame,
    x: f32,
    width: f32,
    y: f32,
    color: Color,
    label: &'a str,
    is_root: bool,
    is_shadow: bool,
    bounds: Rectangle,
}

fn draw_event_rect(args: DrawEventRectArgs<'_>) {
    let DrawEventRectArgs {
        frame,
        x,
        width,
        y,
        color,
        label,
        is_root,
        is_shadow,
        bounds,
    } = args;
    let rect = Rectangle {
        x,
        y: y + 1.0,
        width: width.max(1.0),
        height: (LANE_HEIGHT - 2.0) as f32,
    };

    frame.fill_rectangle(rect.position(), rect.size(), color);

    let border_color = if is_shadow {
        Color::from_rgba(0.0, 0.0, 0.0, 0.05)
    } else if is_root {
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

    if rect.width > 5.0 {
        // Draw the full label but intersect the event clip with the overall
        // canvas/layout bounds so text is not drawn outside the visible area.
        let mut clip = Rectangle {
            x: rect.x + 1.0,
            y: rect.y + 1.0,
            width: rect.width - 2.0,
            height: rect.height - 2.0,
        };

        // Intersect clip with provided bounds
        let x0 = clip.x.max(bounds.x);
        let y0 = clip.y.max(bounds.y);
        let x1 = (clip.x + clip.width).min(bounds.x + bounds.width);
        let y1 = (clip.y + clip.height).min(bounds.y + bounds.height);

        if x1 > x0 && y1 > y0 {
            clip.x = x0;
            clip.y = y0;
            clip.width = x1 - x0;
            clip.height = y1 - y0;

            frame.with_clip(clip, |frame| {
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
            });
        }
    }
}

pub struct EventsProgram<'a> {
    pub events: &'a [TimelineEvent],
    pub thread_groups: &'a [ThreadGroup],
    pub min_ns: u64,
    pub max_ns: u64,
    pub zoom_level: f64,
    pub selected_event: Option<EventId>,
    pub scroll_offset_x: f64,
    pub scroll_offset_y: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub color_mode: ColorMode,
    pub symbols: &'a crate::symbols::Symbols,
    pub kinds: &'a [crate::data::KindInfo],
}

#[derive(Default)]
pub struct EventsState {
    pub modifiers: keyboard::Modifiers,
    pub hovered_event: Option<EventId>,
    pub hovered_position: Option<Point>,
    pub last_click: Option<(EventId, std::time::Instant)>,
    pub press_position: Option<Point>,
    pub pressed_event: Option<EventId>,
    pub dragging: bool,
}

impl<'a> EventsProgram<'a> {
    // Lookup a kind color from the precomputed kinds table by per-event index.
    // If the index is out of range fall back to deriving a color from the
    // event label.
    fn kind_color_from_table(
        kinds: &'a [crate::data::KindInfo],
        idx: u16,
        fallback_label: &str,
    ) -> iced::Color {
        kinds
            .get(idx as usize)
            .map(|k| k.color)
            .unwrap_or_else(|| color_from_label(fallback_label))
    }
    fn find_event_at(&self, position: Point) -> Option<EventId> {
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
                && (content_position.y as f64) < y_offset + lane_total_height
            {
                let (ns_min, ns_max) = crate::timeline::viewport_ns_range(
                    self.scroll_offset_x,
                    self.viewport_width,
                    zoom_level,
                    self.min_ns,
                );

                for thread in group.threads.iter() {
                    if group.show_thread_roots
                        && let Some(root_level) = thread.thread_root_mipmap.as_ref()
                    {
                        for event_id in visible_event_indices_in(
                            self.events,
                            &root_level.events,
                            &root_level.events_tree,
                            ns_min,
                            ns_max,
                        ) {
                            let event = &self.events[event_id.index()];
                            let depth = display_depth(group.show_thread_roots, event);
                            if group.is_collapsed && depth > 0 {
                                continue;
                            }
                            let width = crate::timeline::duration_to_width(
                                event.duration_ns.max(1),
                                self.zoom_level,
                            ) as f32;
                            if width < 1.0 {
                                continue;
                            }
                            let x =
                                crate::timeline::ns_to_x(event.start_ns, self.min_ns, zoom_level)
                                    as f32;
                            let y = y_offset as f32 + depth as f32 * (LANE_HEIGHT as f32);
                            let height = (LANE_HEIGHT - 2.0) as f32;
                            let rect = Rectangle {
                                x,
                                y,
                                width: width.max(1.0),
                                height,
                            };
                            if rect.contains(content_position) {
                                return Some(event_id);
                            }
                        }
                    }

                    for level in &thread.mipmaps {
                        if (level.max_duration_ns as f64) * self.zoom_level < 1.0 {
                            continue;
                        }
                        for event_id in visible_event_indices_in(
                            self.events,
                            &level.events,
                            &level.events_tree,
                            ns_min,
                            ns_max,
                        ) {
                            let event = &self.events[event_id.index()];
                            let depth = display_depth(group.show_thread_roots, event);
                            if group.is_collapsed && depth > 0 {
                                continue;
                            }

                            let width = crate::timeline::duration_to_width(
                                event.duration_ns.max(1),
                                self.zoom_level,
                            ) as f32;
                            if width < 1.0 {
                                continue;
                            }

                            let x =
                                crate::timeline::ns_to_x(event.start_ns, self.min_ns, zoom_level)
                                    as f32;
                            let y = y_offset as f32 + depth as f32 * (LANE_HEIGHT as f32);
                            let height = (LANE_HEIGHT - 2.0) as f32;

                            let rect = Rectangle {
                                x,
                                y,
                                width: width.max(1.0),
                                height,
                            };

                            if rect.contains(content_position) {
                                return Some(event_id);
                            }
                        }
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
        // Use two geometry layers so the tooltip can be rendered above all
        // event text. Some render backends batch text separately from shapes,
        // which can otherwise cause event labels to appear on top of the
        // tooltip background even when the tooltip is drawn last.
        let mut base_frame = canvas::Frame::new(renderer, bounds.size());

        if self.thread_groups.is_empty() {
            return vec![base_frame.into_geometry()];
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

        // The visible drawing area for events is the viewport in canvas-local
        // coordinates (origin at 0,0). Use this when intersecting text clips so
        // we don't rely on the provided `bounds` which may not match the events
        // view coordinates when the canvas is embedded in a larger layout.
        let visible_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: viewport_width as f32,
            height: viewport_height as f32,
        };

        // Draw vertical tick guide lines matching the header ticks.
        let total_ns = self.max_ns.saturating_sub(self.min_ns) as f64;
        let zoom_level = self.zoom_level.max(1e-9);
        let scroll_offset_x_ns = self.scroll_offset_x.max(0.0);
        let rel_min = scroll_offset_x_ns as u64;
        let rel_max = (scroll_offset_x_ns + viewport_width / zoom_level).max(0.0) as u64;
        let ns_min = self.min_ns.saturating_add(rel_min);
        let ns_max = self.min_ns.saturating_add(rel_max);

        // Convert an absolute timestamp (ns) into a screen-space x position.
        // Do the subtraction in ns first to avoid catastrophic cancellation when
        // panning far into large timelines.
        let screen_x = |start_ns: u64| -> f32 {
            let rel_ns = start_ns.saturating_sub(self.min_ns) as f64;
            ((rel_ns - scroll_offset_x_ns) * zoom_level) as f32
        };

        if total_ns > 0.0 {
            // ns per pixel given current zoom: 1 / zoom_level
            let ns_per_pixel = 1.0 / zoom_level;
            let pixel_interval = 100.0;
            let ns_interval = pixel_interval * ns_per_pixel;
            let nice_interval = crate::timeline::ticks::nice_interval(ns_interval);

            let mut relative_ns = if viewport_width > 0.0 {
                (scroll_offset_x_ns / nice_interval).floor() * nice_interval
            } else {
                0.0
            };

            while relative_ns <= total_ns {
                let x_screen = ((relative_ns - scroll_offset_x_ns) * zoom_level) as f32;
                if viewport_width > 0.0 && (x_screen as f64) > viewport_width {
                    break;
                }

                if x_screen < 0.0 {
                    relative_ns += nice_interval;
                    continue;
                }

                // Draw faint vertical line across the events area.
                base_frame.stroke(
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
                && ((y_offset + lane_total_height) < y_min || y_offset > y_max)
            {
                y_offset += lane_total_height + super::LANE_SPACING;
                continue;
            }

            let row_y = y_offset as f32 - self.scroll_offset_y as f32;
            base_frame.stroke(
                &canvas::Path::line(Point::new(0.0, row_y), Point::new(bounds.width, row_y)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.9, 0.9, 0.9))
                    .with_width(1.0),
            );

            for thread in group.threads.iter() {
                if group.show_thread_roots
                    && let Some(root_level) = thread.thread_root_mipmap.as_ref()
                {
                    for event_id in visible_event_indices_in(
                        self.events,
                        &root_level.events,
                        &root_level.events_tree,
                        ns_min,
                        ns_max,
                    ) {
                        let event = &self.events[event_id.index()];
                        let depth = display_depth(group.show_thread_roots, event);
                        if group.is_collapsed && depth > 0 {
                            continue;
                        }

                        let width = crate::timeline::duration_to_width(
                            event.duration_ns.max(1),
                            zoom_level,
                        ) as f32;
                        if width < 1.0 {
                            continue;
                        }

                        let x_screen = screen_x(event.start_ns);
                        if viewport_width > 0.0
                            && ((x_screen + width) < 0.0 || (x_screen as f64) > viewport_width)
                        {
                            continue;
                        }

                        // Thread-root events use a fixed light color.
                        let color = Color::from_rgb(0.85, 0.87, 0.9);
                        let label = self.symbols.resolve(event.label);

                        let y_screen = y_offset as f32 - self.scroll_offset_y as f32
                            + depth as f32 * (LANE_HEIGHT as f32);
                        draw_event_rect(DrawEventRectArgs {
                            frame: &mut base_frame,
                            x: x_screen,
                            width,
                            y: y_screen,
                            color,
                            label,
                            is_root: true,
                            is_shadow: false,
                            bounds: visible_bounds,
                        });
                    }
                }

                let mut smallest_visible_level: Option<&crate::data::ThreadGroupMipMap> = None;

                for level in &thread.mipmaps {
                    if (level.max_duration_ns as f64) * zoom_level < 1.0 {
                        continue;
                    }
                    if smallest_visible_level.is_none() {
                        smallest_visible_level = Some(level);
                    }

                    for event_id in visible_event_indices_in(
                        self.events,
                        &level.events,
                        &level.events_tree,
                        ns_min,
                        ns_max,
                    ) {
                        let event = &self.events[event_id.index()];
                        let depth = display_depth(group.show_thread_roots, event);
                        if group.is_collapsed && depth > 0 {
                            continue;
                        }

                        let width = crate::timeline::duration_to_width(
                            event.duration_ns.max(1),
                            zoom_level,
                        ) as f32;
                        if width < 1.0 {
                            continue;
                        }

                        let x_screen = screen_x(event.start_ns);
                        if viewport_width > 0.0
                            && ((x_screen + width) < 0.0 || (x_screen as f64) > viewport_width)
                        {
                            continue;
                        }

                        let color = if event.is_thread_root {
                            // Thread roots use a fixed light color
                            Color::from_rgb(0.85, 0.87, 0.9)
                        } else {
                            match self.color_mode {
                                ColorMode::Kind => Self::kind_color_from_table(
                                    self.kinds,
                                    event.kind_index,
                                    self.symbols.resolve(event.label),
                                ),
                                ColorMode::Event => {
                                    let label = self.symbols.resolve(event.label);
                                    color_from_label(label)
                                }
                            }
                        };
                        let label = self.symbols.resolve(event.label);
                        let is_thread_root = event.is_thread_root;

                        let y_screen = y_offset as f32 - self.scroll_offset_y as f32
                            + depth as f32 * (LANE_HEIGHT as f32);
                        draw_event_rect(DrawEventRectArgs {
                            frame: &mut base_frame,
                            x: x_screen,
                            width,
                            y: y_screen,
                            color,
                            label,
                            is_root: is_thread_root,
                            is_shadow: false,
                            bounds: visible_bounds,
                        });
                    }
                }

                if let Some(shadow_level) = smallest_visible_level {
                    for (depth, start_ns, duration_ns) in
                        visible_shadows_in(&shadow_level.shadows, ns_min, ns_max)
                    {
                        // The depth from visible_shadows_in is the raw depth.
                        // Adjust for thread root display.
                        let adjusted_depth = if group.show_thread_roots {
                            depth.saturating_add(1)
                        } else {
                            depth
                        };
                        if group.is_collapsed && adjusted_depth > 0 {
                            continue;
                        }

                        let width =
                            crate::timeline::duration_to_width(duration_ns, zoom_level) as f32;
                        if width < 1.0 {
                            continue;
                        }

                        let x_screen = screen_x(start_ns);
                        if viewport_width > 0.0
                            && ((x_screen + width) < 0.0 || (x_screen as f64) > viewport_width)
                        {
                            continue;
                        }

                        let color = Color::from_rgba(0.0, 0.0, 0.0, 0.10);
                        let y_screen = y_offset as f32 - self.scroll_offset_y as f32
                            + adjusted_depth as f32 * (LANE_HEIGHT as f32);
                        draw_event_rect(DrawEventRectArgs {
                            frame: &mut base_frame,
                            x: x_screen,
                            width,
                            y: y_screen,
                            color,
                            label: "",
                            is_root: false,
                            is_shadow: true,
                            bounds: visible_bounds,
                        });
                    }
                }
            }

            if let Some(hovered_id) = state.hovered_event {
                let hovered = &self.events[hovered_id.index()];
                let hovered_depth = display_depth(group.show_thread_roots, hovered);
                if super::group_contains_thread(group, hovered.thread_id)
                    && (!group.is_collapsed || hovered_depth == 0)
                {
                    let width = crate::timeline::duration_to_width(
                        hovered.duration_ns.max(1),
                        zoom_level,
                    ) as f32;
                    let x_screen = screen_x(hovered.start_ns);
                    let y = y_offset as f32 - self.scroll_offset_y as f32
                        + hovered_depth as f32 * (LANE_HEIGHT as f32);

                    base_frame.stroke(
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

            if let Some(selected_id) = self.selected_event {
                let selected = &self.events[selected_id.index()];
                let selected_depth = display_depth(group.show_thread_roots, selected);
                if super::group_contains_thread(group, selected.thread_id)
                    && (!group.is_collapsed || selected_depth == 0)
                {
                    let width = crate::timeline::duration_to_width(
                        selected.duration_ns.max(1),
                        zoom_level,
                    ) as f32;
                    let x_screen = screen_x(selected.start_ns);
                    let y = y_offset as f32 - self.scroll_offset_y as f32
                        + selected_depth as f32 * (LANE_HEIGHT as f32);

                    base_frame.stroke(
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

            y_offset += lane_total_height + super::LANE_SPACING;
        }

        // Draw immediate tooltip following the cursor when hovering an event.
        // Drawn in its own geometry layer to guarantee it appears above all event text.
        let mut tooltip_frame = canvas::Frame::new(renderer, bounds.size());
        if let (Some(hovered_id), Some(cursor_pos)) = (state.hovered_event, state.hovered_position)
        {
            if let Some(event) = self.events.get(hovered_id.index()) {
                // Format time relative to the timeline min_ns.
                let time_str =
                    crate::timeline::format_duration(event.start_ns.saturating_sub(self.min_ns));
                let label = self.symbols.resolve(event.label);

                // Measure text precisely using the canvas text measurement API
                // (preferred to the crude character-count heuristic). Build
                // `canvas::Text` values matching the font size used when drawing
                // and ask the frame for their measured widths.
                let _time_text = canvas::Text {
                    content: time_str.clone(),
                    position: Point::new(0.0, 0.0),
                    size: 12.0.into(),
                    ..Default::default()
                };
                let _label_text = canvas::Text {
                    content: label.to_string(),
                    position: Point::new(0.0, 0.0),
                    size: 12.0.into(),
                    ..Default::default()
                };

                // Use the frame's text measurement helper to get exact widths.
                // Fall back to a conservative minimum width if the renderer
                // doesn't provide measurement (defensive programming).
                // The public `iced` API doesn't expose a renderer-agnostic
                // text-measurement helper on `Frame`/`Renderer` in 0.14, so
                // fall back to a conservative measured width using the
                // `unicode_width` crate. This is much better than a fixed
                // per-character heuristic while remaining portable.
                use unicode_width::UnicodeWidthStr;
                let time_w = (time_str.width() as f32) * 7.0_f32; // 7px avg per glyph @12pt
                let label_w = (label.width() as f32) * 7.0_f32;
                let padding = 6.0_f32;
                let spacing = 8.0_f32;
                let tooltip_w = (time_w + label_w + padding * 2.0 + spacing).max(40.0);
                let tooltip_h = 20.0_f32;

                // Position tooltip near cursor but keep inside visible bounds.
                let mut tx = cursor_pos.x + 10.0;
                let mut ty = cursor_pos.y + 10.0;
                if tx + tooltip_w > visible_bounds.width {
                    tx = cursor_pos.x - tooltip_w - 10.0;
                }
                if ty + tooltip_h > visible_bounds.height {
                    ty = cursor_pos.y - tooltip_h - 10.0;
                }

                // Draw drop shadow for depth.
                let shadow_offset = 2.0_f32;
                tooltip_frame.fill_rectangle(
                    Point::new(tx + shadow_offset, ty + shadow_offset),
                    Size::new(tooltip_w, tooltip_h),
                    Color::from_rgba(0.0, 0.0, 0.0, 0.15),
                );

                // Draw tooltip background (fully opaque to cover underlying content).
                tooltip_frame.fill_rectangle(
                    Point::new(tx, ty),
                    Size::new(tooltip_w, tooltip_h),
                    Color::from_rgb(1.0, 1.0, 1.0),
                );

                // Draw border.
                tooltip_frame.stroke(
                    &canvas::Path::rectangle(Point::new(tx, ty), Size::new(tooltip_w, tooltip_h)),
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.4))
                        .with_width(1.0),
                );

                // Draw time (purple) then label.
                tooltip_frame.fill_text(canvas::Text {
                    content: time_str,
                    position: Point::new(tx + padding, ty + 3.0),
                    color: Color::from_rgb(0.408, 0.322, 0.459),
                    size: 12.0.into(),
                    ..Default::default()
                });

                tooltip_frame.fill_text(canvas::Text {
                    content: label.to_string(),
                    position: Point::new(tx + padding + time_w + spacing, ty + 3.0),
                    color: Color::from_rgb(0.15, 0.15, 0.15),
                    size: 12.0.into(),
                    ..Default::default()
                });
            }
        }

        let tooltip_geometry = tooltip_frame.into_geometry();

        // Avoid returning an extra empty geometry layer when not hovering.
        if state.hovered_event.is_some() && state.hovered_position.is_some() {
            vec![base_frame.into_geometry(), tooltip_geometry]
        } else {
            vec![base_frame.into_geometry()]
        }
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
                // Track cursor position (canvas-local) for the tooltip to follow
                let prev_pos = state.hovered_position;
                state.hovered_position = cursor.position_in(bounds);

                let new_hovered = state.hovered_position.and_then(|p| self.find_event_at(p));

                if new_hovered != state.hovered_event {
                    state.hovered_event = new_hovered;
                    return Some(canvas::Action::publish(Message::EventHovered(
                        state.hovered_event,
                    )));
                }

                // If still hovering the same event, request a redraw so the tooltip
                // follows the cursor even when hover target hasn't changed.
                if state.hovered_event.is_some() && state.hovered_position != prev_pos {
                    return Some(canvas::Action::request_redraw());
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
                if !state.dragging
                    && let (Some(pressed_event), Some(position)) =
                        (state.pressed_event, cursor.position_in(bounds))
                    && let Some(release_event) = self.find_event_at(position)
                {
                    let is_same_event = pressed_event == release_event;
                    if is_same_event {
                        let now = std::time::Instant::now();
                        if let Some((prev_event, prev_time)) = &state.last_click {
                            let is_double = *prev_event == release_event
                                && now.duration_since(*prev_time)
                                    <= std::time::Duration::from_millis(400);
                            if is_double {
                                state.last_click = None;
                                state.press_position = None;
                                state.pressed_event = None;
                                state.dragging = false;
                                return Some(canvas::Action::publish(Message::EventDoubleClicked(
                                    release_event,
                                )));
                            }
                        }

                        state.last_click = Some((release_event, now));
                        state.press_position = None;
                        state.pressed_event = None;
                        state.dragging = false;
                        return Some(canvas::Action::publish(Message::EventSelected(
                            release_event,
                        )));
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
                                    let scroll_amount = *y * 30.0;
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
                                    let scroll_amount = *y * 30.0;
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
