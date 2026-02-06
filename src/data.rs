use crate::timeline::{self, ThreadGroup, TimelineData};
use iced::Color;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(pub u32);

impl EventId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEvent {
    pub label: crate::symbols::Symbol,
    pub start_ns: u64,
    pub duration_ns: u64,
    pub depth: u32,
    pub thread_id: u32,
    pub event_kind: crate::symbols::Symbol,
    pub additional_data: Vec<crate::symbols::Symbol>,
    pub payload_integer: Option<u64>,
    pub color: Color,
    pub is_thread_root: bool,
}

#[derive(Debug, Clone)]
pub struct ThreadData {
    pub thread_id: u32,
    pub events: Vec<EventId>,
    pub thread_root: Option<EventId>,
}

use analyzeme::ProfilingData;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FileData {
    pub event_count: usize,
    pub cmd: String,
    pub pid: u32,
    pub timeline: TimelineData,
    pub events: Vec<TimelineEvent>,
    pub merged_thread_groups: Vec<ThreadGroup>,
    // Simple symbol interner for event strings so we store compact symbol ids
    // in events rather than repeated Strings.
    pub symbols: crate::symbols::Symbols,
}

#[derive(Debug, Clone)]
pub struct FileUi {
    pub color_mode: timeline::ColorMode,
    pub selected_event: Option<EventId>,
    pub hovered_event: Option<EventId>,
    pub merge_threads: bool,
    pub initial_fit_done: bool,
    pub view_type: crate::ViewType,
    // Use f64 for zoom/scroll state to avoid precision loss at high zoom.
    pub zoom_level: f64,
    /// Horizontal scroll offset in nanoseconds, relative to timeline.min_ns.
    pub scroll_offset_x: f64,
    pub scroll_offset_y: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
}

impl Default for FileUi {
    fn default() -> Self {
        FileUi {
            color_mode: timeline::ColorMode::default(),
            selected_event: None,
            hovered_event: None,
            merge_threads: true,
            initial_fit_done: false,
            view_type: crate::ViewType::default(),
            zoom_level: 1.0_f64,
            scroll_offset_x: 0.0_f64,
            scroll_offset_y: 0.0_f64,
            viewport_width: 0.0_f64,
            viewport_height: 0.0_f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileTab {
    pub data: FileData,
    // UI/state fields that are only meaningful once the file is loaded.
    pub ui: FileUi,
    pub load_duration_ns: Option<u64>,
}

pub fn load_profiling_data(path: &Path) -> Result<FileTab, String> {
    let data = load_profiling_source(path)?;
    let metadata = data.metadata();
    let metadata_start_ns = metadata
        .start_time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    // Create the symbol interner first and intern strings as we parse events so
    // we avoid allocating duplicate Strings for every parsed event.
    let mut symbols = crate::symbols::Symbols::new();
    let collected = collect_timeline_events(&data, &mut symbols, metadata_start_ns);
    let kind_color_map = build_kind_color_map(&collected.events, &symbols);
    let mut events = collected.events;
    apply_kind_colors(&mut events, &kind_color_map);
    let mut threads = build_threads_index(&events);
    assign_event_depths(&mut events, &mut threads);
    let thread_data_vec = build_thread_data(&mut events, threads, &mut symbols);
    let thread_groups = build_thread_groups(&mut events, &thread_data_vec);
    let merged_thread_groups = build_merged_thread_groups(&mut events, &thread_data_vec);

    Ok(FileTab {
        data: FileData {
            event_count: collected.event_count,
            cmd: metadata.cmd.clone(),
            pid: metadata.process_id,
            timeline: TimelineData {
                thread_groups,
                min_ns: 0,
                max_ns: collected.max_ns,
            },
            events,
            merged_thread_groups,
            symbols,
        },
        ui: FileUi::default(),
        load_duration_ns: None,
    })
}

#[derive(Debug)]
struct CollectedEvents {
    events: Vec<TimelineEvent>,
    max_ns: u64,
    event_count: usize,
}

fn load_profiling_source(path: &Path) -> Result<ProfilingData, String> {
    let stem = path.with_extension("");
    ProfilingData::new(&stem)
        .map_err(|e| format!("Failed to load profiling data from {:?}: {}", stem, e))
}

fn collect_timeline_events(
    data: &ProfilingData,
    symbols: &mut crate::symbols::Symbols,
    metadata_start_ns: u64,
) -> CollectedEvents {
    let mut events = Vec::new();
    let mut max_ns: u64 = 0;
    let mut event_count: usize = 0;

    for lightweight_event in data.iter() {
        let event = data.to_full_event(&lightweight_event);
        let thread_id = event.thread_id;

        if let analyzeme::EventPayload::Timestamp(analyzeme::Timestamp::Interval { start, end }) =
            &event.payload
        {
            event_count += 1;

            let start_ns = (start
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64)
                .saturating_sub(metadata_start_ns);
            let end_ns = (end
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64)
                .saturating_sub(metadata_start_ns);

            max_ns = max_ns.max(end_ns);

            events.push(TimelineEvent {
                thread_id,
                label: symbols.intern(event.label.as_ref()),
                start_ns,
                duration_ns: end_ns.saturating_sub(start_ns),
                depth: 0,
                event_kind: symbols.intern(event.event_kind.as_ref()),
                additional_data: event
                    .additional_data
                    .iter()
                    .map(|s| symbols.intern(s.as_ref()))
                    .collect::<Vec<_>>(),
                payload_integer: event.payload.integer(),
                // Filled in after we know all kinds.
                color: timeline::color_from_hsl(0.0, 0.0, 0.85),
                is_thread_root: false,
            });
        }
    }

    CollectedEvents {
        events,
        max_ns,
        event_count,
    }
}

fn build_kind_color_map(
    events: &[TimelineEvent],
    symbols: &crate::symbols::Symbols,
) -> HashMap<crate::symbols::Symbol, Color> {
    // Collect unique kinds into a HashSet to remove duplicates, then sort the
    // unique kinds by their resolved string so coloring is deterministic.
    let mut kinds_set: std::collections::HashSet<crate::symbols::Symbol> =
        events.iter().map(|e| e.event_kind).collect();
    let mut kinds: Vec<crate::symbols::Symbol> = kinds_set.drain().collect();
    // Avoid allocating a String when sorting: resolve returns &str so we can
    // use it directly as the sort key.
    kinds.sort_by_key(|s| symbols.resolve(*s));

    let kind_count = kinds.len().max(1);

    let mut kind_color_map: HashMap<crate::symbols::Symbol, Color> = HashMap::new();
    let base_hue = 120.0_f32;
    for (i, &kind_sym) in kinds.iter().enumerate() {
        let step = 360.0 / kind_count as f32;
        let hue = (base_hue + (i as f32) * step) % 360.0;
        let color = timeline::color_from_hsl(hue, 0.35, 0.8);
        kind_color_map.insert(kind_sym, color);
    }

    kind_color_map
}

fn apply_kind_colors(
    events: &mut [TimelineEvent],
    kind_color_map: &HashMap<crate::symbols::Symbol, Color>,
) {
    for event in events {
        event.color = kind_color_map
            .get(&event.event_kind)
            .cloned()
            .unwrap_or_else(|| timeline::color_from_hsl(0.0, 0.0, 0.85));
    }
}

fn build_threads_index(events: &[TimelineEvent]) -> HashMap<u32, Vec<EventId>> {
    let mut threads: HashMap<u32, Vec<EventId>> = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        threads
            .entry(event.thread_id)
            .or_default()
            .push(EventId(index as u32));
    }
    threads
}

fn assign_event_depths(events: &mut [TimelineEvent], threads: &mut HashMap<u32, Vec<EventId>>) {
    for thread_events in threads.values_mut() {
        thread_events.sort_by_key(|event_id| events[event_id.index()].start_ns);
        let mut stack: Vec<u64> = Vec::new();
        for event_id in thread_events.iter() {
            let event = &mut events[event_id.index()];
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
}

fn build_thread_data(
    events: &mut Vec<TimelineEvent>,
    threads: HashMap<u32, Vec<EventId>>,
    symbols: &mut crate::symbols::Symbols,
) -> Vec<Arc<ThreadData>> {
    let mut thread_data_vec = Vec::new();
    for (thread_id, event_ids) in threads {
        let thread_root = build_thread_root(events, thread_id, &event_ids, symbols);
        thread_data_vec.push(Arc::new(ThreadData {
            thread_id,
            events: event_ids,
            thread_root,
        }));
    }

    thread_data_vec.sort_by_key(|t| t.thread_id);
    thread_data_vec
}

fn build_thread_groups(
    events: &mut [TimelineEvent],
    thread_data: &[Arc<ThreadData>],
) -> Vec<ThreadGroup> {
    let mut thread_groups = Vec::new();
    for thread in thread_data {
        let threads = Arc::new(vec![thread.clone()]);
        let (_events, max_depth, mipmaps) =
            timeline::build_thread_group_events(events, &threads, false);
        thread_groups.push(ThreadGroup {
            threads,
            mipmaps,
            max_depth,
            is_collapsed: false,
            show_thread_roots: false,
        });
    }

    thread_groups
}

fn build_merged_thread_groups(
    events: &mut [TimelineEvent],
    threads: &[Arc<ThreadData>],
) -> Vec<ThreadGroup> {
    if threads.is_empty() {
        return Vec::new();
    }

    let mut intervals: Vec<(usize, u64, u64, Arc<ThreadData>)> = threads
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, thread)| {
            let mut start = u64::MAX;
            let mut end = 0u64;
            for event_id in &thread.events {
                let event = &events[event_id.index()];
                start = start.min(event.start_ns);
                end = end.max(event.start_ns.saturating_add(event.duration_ns));
            }
            if start == u64::MAX {
                start = 0;
            }
            (index, start, end, thread)
        })
        .collect();

    intervals.sort_by(|(_, a_start, a_end, _), (_, b_start, b_end, _)| {
        let a_len = a_end.saturating_sub(*a_start);
        let b_len = b_end.saturating_sub(*b_start);
        a_len.cmp(&b_len).then_with(|| a_start.cmp(b_start))
    });

    let mut groups: Vec<Vec<(usize, Arc<ThreadData>)>> = Vec::new();
    let mut group_ranges: Vec<(u64, u64)> = Vec::new();

    for (index, start, end, thread) in intervals {
        let mut placed = false;
        for (group, (group_start, group_end)) in groups.iter_mut().zip(group_ranges.iter_mut()) {
            let overlaps = start < *group_end && end > *group_start;
            if !overlaps {
                *group_start = (*group_start).min(start);
                *group_end = (*group_end).max(end);
                group.push((index, thread.clone()));
                placed = true;
                break;
            }
        }

        if !placed {
            groups.push(vec![(index, thread)]);
            group_ranges.push((start, end));
        }
    }

    groups.sort_by_key(|group| {
        group
            .iter()
            .map(|(index, _)| *index)
            .min()
            .unwrap_or(usize::MAX)
    });

    let mut thread_groups = Vec::new();
    for group in groups {
        let mut group = group;
        group.sort_by_key(|(index, _)| *index);
        let threads = Arc::new(
            group
                .into_iter()
                .map(|(_, thread)| thread)
                .collect::<Vec<_>>(),
        );
        let show_thread_roots = threads.len() > 1;
        let (_events, max_depth, mipmaps) =
            timeline::build_thread_group_events(events, &threads, show_thread_roots);
        thread_groups.push(ThreadGroup {
            threads,
            mipmaps,
            max_depth,
            is_collapsed: false,
            show_thread_roots,
        });
    }

    thread_groups
}

fn build_thread_root(
    events: &mut Vec<TimelineEvent>,
    thread_id: u32,
    event_ids: &[EventId],
    symbols: &mut crate::symbols::Symbols,
) -> Option<EventId> {
    let mut start_ns = u64::MAX;
    let mut end_ns = 0u64;

    for event_id in event_ids {
        let event = &events[event_id.index()];
        start_ns = start_ns.min(event.start_ns);
        end_ns = end_ns.max(event.start_ns.saturating_add(event.duration_ns));
    }

    if start_ns == u64::MAX {
        return None;
    }

    let event_id = EventId(events.len() as u32);
    let event = TimelineEvent {
        label: symbols.intern(&format!("Thread {}", thread_id)),
        start_ns,
        duration_ns: end_ns.saturating_sub(start_ns),
        depth: 0,
        thread_id,
        event_kind: symbols.intern("Thread"),
        additional_data: Vec::new(),
        payload_integer: None,
        color: Color::from_rgb(0.85, 0.87, 0.9),
        is_thread_root: true,
    };
    events.push(event);

    Some(event_id)
}

pub fn format_panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        format!("Loading thread panicked: {}", message)
    } else if let Some(message) = payload.downcast_ref::<String>() {
        format!("Loading thread panicked: {}", message)
    } else {
        "Loading thread panicked with unknown payload".to_string()
    }
}
