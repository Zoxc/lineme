use iced::widget::{button, column, container, pick_list, row, scrollable, text, Space, slider};
use iced::widget::canvas::{self, Action, Canvas, Geometry, Program};
use iced::mouse::Cursor;
use iced::{Alignment, Element, Length, Task, Color, Point, Rectangle, Renderer, Size, Padding};
use iced_aw::{tab_bar, TabLabel};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use analyzeme::ProfilingData;

pub fn main() -> iced::Result {
    iced::application(Lineme::new, Lineme::update, Lineme::view)
        .title(Lineme::title)
        .run()
}

#[derive(Debug, Clone)]
struct TimelineEvent {
    label: String,
    start_ns: u64,
    duration_ns: u64,
    depth: u32,
    thread_id: u64,
    color: Color,
}

#[derive(Debug, Clone)]
struct ThreadData {
    thread_id: u64,
    events: Vec<TimelineEvent>,
    max_depth: u32,
}

#[derive(Debug, Clone, Default)]
struct TimelineData {
    threads: Vec<ThreadData>,
    min_ns: u64,
    max_ns: u64,
}

#[derive(Debug, Clone)]
struct Stats {
    event_count: usize,
    cmd: String,
    pid: u32,
    timeline: TimelineData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ViewType {
    Stats,
    #[default]
    Timeline,
}

impl ViewType {
    const ALL: [ViewType; 2] = [ViewType::Stats, ViewType::Timeline];
}

impl std::fmt::Display for ViewType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewType::Stats => write!(f, "Stats"),
            ViewType::Timeline => write!(f, "Timeline"),
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    OpenFile,
    FileSelected(PathBuf),
    FileLoaded(PathBuf, Stats),
    ErrorOccurred(String),
    ViewChanged(ViewType),
    CloseTab(usize),
    OpenSettings,
    EventSelected(TimelineEvent),
    TimelineZoomed { delta: f32, x: f32, width: f32 },
    TimelineScrolled { delta_x: f32, width: f32 },
    TimelinePanned(u64),
    TimelineDragPanned { delta_x: f32, width: f32 },
    None,
}

struct Lineme {
    active_tab: usize,
    files: Vec<FileData>,
    show_settings: bool,
    #[allow(dead_code)]
    settings: SettingsPage,
}

struct FileData {
    path: PathBuf,
    stats: Stats,
    view_type: ViewType,
    selected_event: Option<TimelineEvent>,
    view_start_ns: u64,
    view_end_ns: u64,
}

struct SettingsPage {
    #[allow(dead_code)]
    show_details: bool,
}

impl Lineme {
    fn new() -> (Self, Task<Message>) {
        let mut initial_task = Task::none();
        
        if let Some(path_str) = std::env::args().nth(1) {
            let path = PathBuf::from(path_str);
            initial_task = Task::perform(
                async move {
                    match load_profiling_data(&path) {
                        Ok(stats) => Message::FileLoaded(path, stats),
                        Err(e) => Message::ErrorOccurred(e),
                    }
                },
                |msg| msg,
            );
        }

        (
            Lineme {
                active_tab: 0,
                files: Vec::new(),
                show_settings: false,
                settings: SettingsPage { show_details: true },
            },
            initial_task,
        )
    }

    fn title(&self) -> String {
        if self.show_settings {
            return "Lineme - Settings".to_string();
        }
        if let Some(file) = self.files.get(self.active_tab) {
            file.path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Lineme".to_string())
        } else {
            "Lineme - measureme profdata viewer".to_string()
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(index) => {
                self.active_tab = index;
                self.show_settings = false;
            }
            Message::OpenFile => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("measureme profdata", &["mm_profdata"])
                            .pick_file()
                            .await
                    },
                    |file_handle| {
                        if let Some(handle) = file_handle {
                            Message::FileSelected(handle.path().to_path_buf())
                        } else {
                            Message::None
                        }
                    },
                );
            }
            Message::FileSelected(path) => {
                return Task::perform(
                    async move {
                        match load_profiling_data(&path) {
                            Ok(stats) => Message::FileLoaded(path, stats),
                            Err(e) => Message::ErrorOccurred(e),
                        }
                    },
                    |msg| msg,
                );
            }
            Message::FileLoaded(path, stats) => {
                let view_start_ns = stats.timeline.min_ns;
                let view_end_ns = stats.timeline.max_ns;
                self.files.push(FileData { 
                    path, 
                    stats,
                    view_type: ViewType::default(),
                    selected_event: None,
                    view_start_ns,
                    view_end_ns,
                });
                self.active_tab = self.files.len() - 1;
                self.show_settings = false;
            }
            Message::ErrorOccurred(e) => {
                eprintln!("Error: {}", e);
            }
            Message::ViewChanged(view) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.view_type = view;
                }
            }
            Message::CloseTab(index) => {
                if index < self.files.len() {
                    self.files.remove(index);
                    if self.active_tab >= self.files.len() && !self.files.is_empty() {
                        self.active_tab = self.files.len() - 1;
                    }
                }
            }
            Message::OpenSettings => {
                self.show_settings = true;
            }
            Message::EventSelected(event) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.selected_event = Some(event);
                }
            }
            Message::TimelineZoomed { delta, x, width } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let old_duration = file.view_end_ns.saturating_sub(file.view_start_ns) as f64;
                    if old_duration <= 0.0 { return Task::none(); }

                    let zoom_factor = if delta > 0.0 { 0.9 } else { 1.1 };
                    let new_duration = (old_duration * zoom_factor) as u64;
                    
                    let total_duration = file.stats.timeline.max_ns - file.stats.timeline.min_ns;
                    let new_duration = new_duration.clamp(1000, total_duration);
                    
                    let time_at_x = file.view_start_ns as f64 + (x as f64 / width as f64) * old_duration;
                    
                    let new_start = (time_at_x - (x as f64 / width as f64) * new_duration as f64) as i64;
                    
                    file.view_start_ns = new_start.clamp(file.stats.timeline.min_ns as i64, (file.stats.timeline.max_ns - new_duration) as i64) as u64;
                    file.view_end_ns = file.view_start_ns + new_duration;
                }
            }
            Message::TimelineScrolled { delta_x, width } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let duration = file.view_end_ns.saturating_sub(file.view_start_ns);
                    let scroll_amount = (delta_x as f64 / width as f64 * duration as f64) as i64;
                    
                    let new_start = (file.view_start_ns as i64 + scroll_amount)
                        .clamp(file.stats.timeline.min_ns as i64, (file.stats.timeline.max_ns.saturating_sub(duration)) as i64) as u64;
                        
                    file.view_start_ns = new_start;
                    file.view_end_ns = new_start + duration;
                }
            }
            Message::TimelineDragPanned { delta_x, width } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let duration = file.view_end_ns.saturating_sub(file.view_start_ns);
                    let scroll_amount = (delta_x as f64 / width as f64 * duration as f64) as i64;
                    
                    let new_start = (file.view_start_ns as i64 - scroll_amount)
                        .clamp(file.stats.timeline.min_ns as i64, (file.stats.timeline.max_ns.saturating_sub(duration)) as i64) as u64;
                        
                    file.view_start_ns = new_start;
                    file.view_end_ns = new_start + duration;
                }
            }
            Message::TimelinePanned(new_start) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let duration = file.view_end_ns.saturating_sub(file.view_start_ns);
                    file.view_start_ns = new_start;
                    file.view_end_ns = new_start + duration;
                }
            }
            Message::None => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let mut bar = tab_bar::TabBar::new(Message::TabSelected)
            .on_close(Message::CloseTab);

        for (i, file) in self.files.iter().enumerate() {
            let label = file.path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unknown".to_string());
            
            bar = bar.push(i, TabLabel::Text(label));
        }
        
        if !self.files.is_empty() && !self.show_settings {
            bar = bar.set_active_tab(&self.active_tab);
        }

        let header = row![bar, Space::new().width(Length::Fill)];

        let header = if !self.files.is_empty() && !self.show_settings {
            header.push(pick_list(
                &ViewType::ALL[..],
                Some(self.files[self.active_tab].view_type),
                Message::ViewChanged,
            ))
        } else {
            header.push(Space::new().width(Length::Shrink))
        };

        let header = header
            .push(button("Settings").on_press(Message::OpenSettings))
            .push(button("Open").on_press(Message::OpenFile))
            .spacing(10)
            .padding(5)
            .align_y(Alignment::Center);

        let content: Element<'_, Message> = if self.show_settings {
            self.settings_view()
        } else if let Some(file) = self.files.get(self.active_tab) {
            match file.view_type {
                ViewType::Stats => self.file_view(file),
                ViewType::Timeline => self.timeline_view(file),
            }
        } else {
            container(text("Open a file to start").size(20))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        };

        column![header, content].into()
    }

    fn file_view(&self, file: &FileData) -> Element<'_, Message> {
        let content = column![
            text(format!("File: {}", file.path.display())).size(20),
            text(format!("Command: {}", file.stats.cmd)),
            text(format!("PID: {}", file.stats.pid)),
            text(format!("Event count: {}", file.stats.event_count)),
            button("Open another file").on_press(Message::OpenFile),
        ]
        .spacing(10)
        .padding(20);

        scrollable(content).into()
    }

    fn timeline_view<'a>(&self, file: &'a FileData) -> Element<'a, Message> {
        let timeline_data = &file.stats.timeline;
        
        let mut thread_labels = column![].spacing(5);
        let mut timeline_lanes = column![].spacing(5);
        
        let total_ns = timeline_data.max_ns - timeline_data.min_ns;
        if total_ns == 0 {
            return container(text("No events to display"))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        }

        for thread in &timeline_data.threads {
            let lane_height = (thread.max_depth + 1) as f32 * 20.0;
            
            thread_labels = thread_labels.push(
                container(text(format!("Thread {}", thread.thread_id)))
                    .height(Length::Fixed(lane_height))
                    .padding(5)
            );
            
            let lane_canvas = Canvas::new(LaneProgram {
                events: &thread.events,
                view_start_ns: file.view_start_ns,
                view_end_ns: file.view_end_ns,
                selected_event: &file.selected_event,
                thread_id: thread.thread_id,
                max_depth: thread.max_depth,
            })
            .width(Length::Fill)
            .height(Length::Fixed(lane_height));
            
            timeline_lanes = timeline_lanes.push(lane_canvas);
        }

        let main_view = scrollable(row![
            thread_labels.width(Length::Fixed(150.0)),
            timeline_lanes.width(Length::Fill),
        ])
        .height(Length::Fill);

        let duration = file.view_end_ns.saturating_sub(file.view_start_ns);
        let max_scroll = timeline_data.max_ns.saturating_sub(duration);
        
        let scrollbar = if max_scroll > timeline_data.min_ns {
            container(slider(
                timeline_data.min_ns as f64..=max_scroll as f64,
                file.view_start_ns as f64,
                |val| Message::TimelinePanned(val as u64)
            ))
            .padding(Padding { top: 0.0, right: 10.0, bottom: 0.0, left: 150.0 }) // Align with timeline lanes
        } else {
            container(Space::new().height(0))
        };

        let details_panel = if let Some(event) = &file.selected_event {
            container(column![
                text(format!("Event: {}", event.label)).size(20),
                text(format!("Thread: {}", event.thread_id)),
                text(format!("Start: {} ns", event.start_ns)),
                text(format!("Duration: {} ns", event.duration_ns)),
            ].spacing(5).padding(10))
            .width(Length::Fill)
            .height(Length::Fixed(120.0))
        } else {
            container(text("Select an event to see details"))
                .width(Length::Fill)
                .height(Length::Fixed(120.0))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
        };

        column![main_view, scrollbar, details_panel].into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let content = column![
            text("Settings").size(30),
            text("Welcome to Lineme Settings"),
            text(format!("Currently managing {} open files", self.files.len())),
            button("Open file from here").on_press(Message::OpenFile),
        ]
        .spacing(10)
        .padding(20);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .into()
    }
}

struct LaneProgram<'a> {
    events: &'a [TimelineEvent],
    view_start_ns: u64,
    view_end_ns: u64,
    selected_event: &'a Option<TimelineEvent>,
    thread_id: u64,
    max_depth: u32,
}

#[derive(Default)]
struct LaneState {
    drag_start: Option<Point>,
}

impl<'a> Program<Message> for LaneProgram<'a> {
    type State = LaneState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        
        let total_ns = self.view_end_ns.saturating_sub(self.view_start_ns);
        if total_ns == 0 || self.events.is_empty() {
            return vec![frame.into_geometry()];
        }

        let mut last_rects: Vec<Option<(f32, f32, Color)>> = vec![None; (self.max_depth + 1) as usize];

        for event in self.events {
            let event_end_ns = event.start_ns + event.duration_ns;
            if event_end_ns < self.view_start_ns || event.start_ns > self.view_end_ns {
                continue;
            }

            let x = ((event.start_ns as f64 - self.view_start_ns as f64) / total_ns as f64) as f32 * bounds.width;
            let width = (event.duration_ns as f64 / total_ns as f64) as f32 * bounds.width;
            let depth = event.depth as usize;
            let color = event.color;

            if let Some((cur_x, cur_w, cur_color)) = last_rects[depth] {
                let end_x = cur_x + cur_w;
                if color == cur_color && x <= end_x + 0.5 {
                    let new_end = (x + width).max(end_x);
                    last_rects[depth] = Some((cur_x, new_end - cur_x, cur_color));
                    continue;
                } else {
                    // Draw previous
                    let y = depth as f32 * 20.0;
                    frame.fill_rectangle(Point::new(cur_x, y), Size::new(cur_w.max(1.0), 18.0), cur_color);
                }
            }
            last_rects[depth] = Some((x, width, color));
        }

        // Draw remaining rects
        for (depth, rect) in last_rects.into_iter().enumerate() {
            if let Some((cur_x, cur_w, cur_color)) = rect {
                let y = depth as f32 * 20.0;
                frame.fill_rectangle(Point::new(cur_x, y), Size::new(cur_w.max(1.0), 18.0), cur_color);
            }
        }

        let mut geometries = vec![frame.into_geometry()];

        if let Some(selected) = self.selected_event {
            if selected.thread_id == self.thread_id {
                let total_ns = self.view_end_ns.saturating_sub(self.view_start_ns);
                if total_ns > 0 {
                    let mut highlight_frame = canvas::Frame::new(renderer, bounds.size());
                    let x = ((selected.start_ns as f64 - self.view_start_ns as f64) / total_ns as f64) as f32 * bounds.width;
                    let width = (selected.duration_ns as f64 / total_ns as f64) as f32 * bounds.width;
                    let y = selected.depth as f32 * 20.0;

                    highlight_frame.stroke(
                        &canvas::Path::rectangle(Point::new(x, y), Size::new(width.max(1.0), 18.0)),
                        canvas::Stroke::default().with_color(Color::from_rgb(1.0, 1.0, 1.0)).with_width(2.0),
                    );
                    geometries.push(highlight_frame.into_geometry());
                }
            }
        }

        geometries
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let total_ns = self.view_end_ns.saturating_sub(self.view_start_ns);
                    if total_ns == 0 { return None; }

                    let mut hit = false;
                    for event in self.events {
                        let event_end_ns = event.start_ns + event.duration_ns;
                        if event_end_ns < self.view_start_ns || event.start_ns > self.view_end_ns {
                            continue;
                        }

                        let x = ((event.start_ns as f64 - self.view_start_ns as f64) / total_ns as f64) as f32 * bounds.width;
                        let width = (event.duration_ns as f64 / total_ns as f64) as f32 * bounds.width;
                        let y = event.depth as f32 * 20.0;
                        let height = 18.0;

                        let rect = Rectangle {
                            x,
                            y,
                            width: width.max(1.0),
                            height,
                        };

                        if rect.contains(position) {
                            hit = true;
                            return Some(Action::publish(Message::EventSelected(event.clone())));
                        }
                    }

                    if !hit {
                        state.drag_start = Some(position);
                    }
                }
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                state.drag_start = None;
            }
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                if let Some(start_pos) = state.drag_start {
                    if let Some(current_pos) = cursor.position_in(bounds) {
                        let delta_x = current_pos.x - start_pos.x;
                        state.drag_start = Some(current_pos);
                        return Some(Action::publish(Message::TimelineDragPanned {
                            delta_x,
                            width: bounds.width,
                        }));
                    }
                }
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    match delta {
                        iced::mouse::ScrollDelta::Lines { x, y } | iced::mouse::ScrollDelta::Pixels { x, y } => {
                            if y.abs() > x.abs() {
                                // Vertical scroll -> Zoom
                                return Some(Action::publish(Message::TimelineZoomed {
                                    delta: *y,
                                    x: position.x,
                                    width: bounds.width,
                                }));
                            } else {
                                // Horizontal scroll -> Pan
                                return Some(Action::publish(Message::TimelineScrolled {
                                    delta_x: *x,
                                    width: bounds.width,
                                }));
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

fn color_from_label(label: &str) -> Color {
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

fn load_profiling_data(path: &Path) -> Result<Stats, String> {
    let stem = path.with_extension("");
    
    let data = ProfilingData::new(&stem)
        .map_err(|e| format!("Failed to load profiling data from {:?}: {}", stem, e))?;

    let metadata = data.metadata();
    
    let mut threads: HashMap<u64, Vec<TimelineEvent>> = HashMap::new();
    let mut min_ns = u64::MAX;
    let mut max_ns = 0;
    let mut event_count = 0;

    for lightweight_event in data.iter() {
        event_count += 1;
        let event = data.to_full_event(&lightweight_event);
        let thread_id = event.thread_id as u64;

        if let analyzeme::EventPayload::Timestamp(timestamp) = &event.payload {
            let (start_ns, end_ns) = match timestamp {
                analyzeme::Timestamp::Interval { start, end } => {
                    let s = start.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
                    let e = end.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
                    (s, e)
                }
                analyzeme::Timestamp::Instant(t) => {
                    let ns = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
                    (ns, ns)
                }
            };

            min_ns = min_ns.min(start_ns);
            max_ns = max_ns.max(end_ns);

            threads.entry(thread_id).or_default().push(TimelineEvent {
                label: event.label.to_string(),
                start_ns,
                duration_ns: end_ns - start_ns,
                depth: 0,
                thread_id,
                color: color_from_label(&event.label),
            });
        }
    }

    for thread_events in threads.values_mut() {
        thread_events.sort_by_key(|e| e.start_ns);
        let mut stack: Vec<u64> = Vec::new();
        for event in thread_events.iter_mut() {
            let end_ns = event.start_ns + event.duration_ns;
            while let Some(&last_end) = stack.last() {
                if last_end <= event.start_ns {
                    stack.pop();
                } else {
                    break;
                }
            }
            event.depth = stack.len() as u32;
            stack.push(end_ns);
        }
    }
    
    let mut thread_data_vec = Vec::new();
    for (thread_id, events) in threads {
        let max_depth = events.iter().map(|e| e.depth).max().unwrap_or(0);
        thread_data_vec.push(ThreadData {
            thread_id,
            events,
            max_depth,
        });
    }
    
    thread_data_vec.sort_by_key(|t| t.thread_id);

    Ok(Stats {
        event_count,
        cmd: metadata.cmd.clone(),
        pid: metadata.process_id,
        timeline: TimelineData {
            threads: thread_data_vec,
            min_ns: if min_ns == u64::MAX { 0 } else { min_ns },
            max_ns,
        },
    })
}
