#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnalignedU64([u8; 8]);

impl UnalignedU64 {
    pub fn new(v: u64) -> Self {
        Self(v.to_le_bytes())
    }

    pub fn get(&self) -> u64 {
        u64::from_le_bytes(self.0)
    }

    pub fn set(&mut self, v: u64) {
        self.0 = v.to_le_bytes();
    }
}

impl From<u64> for UnalignedU64 {
    fn from(v: u64) -> Self {
        UnalignedU64::new(v)
    }
}

impl From<UnalignedU64> for u64 {
    fn from(v: UnalignedU64) -> Self {
        v.get()
    }
}
use analyzeme::ProfilingData;
use iced::Color;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

// ColorMode, color helper and display_depth are part of the shared public
// API used by UI code. Define them here so data logic doesn't depend on
// timeline UI internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    Kind,
    Event,
}

impl ColorMode {
    pub const ALL: [ColorMode; 2] = [ColorMode::Kind, ColorMode::Event];
}

impl std::fmt::Display for ColorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorMode::Kind => write!(f, "Kind"),
            ColorMode::Event => write!(f, "Event"),
        }
    }
}

pub fn color_from_hsl(h: f32, s: f32, l: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = (h / 60.0) % 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());

    let (r1, g1, b1) = if (0.0..1.0).contains(&h_prime) {
        (c, x, 0.0)
    } else if (1.0..2.0).contains(&h_prime) {
        (x, c, 0.0)
    } else if (2.0..3.0).contains(&h_prime) {
        (0.0, c, x)
    } else if (3.0..4.0).contains(&h_prime) {
        (0.0, x, c)
    } else if (4.0..5.0).contains(&h_prime) {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let m = l - c / 2.0;
    Color::from_rgb(
        (r1 + m).clamp(0.0, 1.0),
        (g1 + m).clamp(0.0, 1.0),
        (b1 + m).clamp(0.0, 1.0),
    )
}

pub fn display_depth(show_thread_roots: bool, event: &TimelineEvent) -> u32 {
    if show_thread_roots && !event.is_thread_root {
        event.depth.saturating_add(1)
    } else {
        event.depth
    }
}

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

// Event-related types are defined below in this file.
pub type ThreadGroupId = Arc<Vec<Arc<ThreadData>>>;
pub type ThreadGroupKey = usize;

#[derive(Debug, Clone)]
pub struct ThreadGroup {
    pub threads: ThreadGroupId,
    pub mipmaps: Vec<ThreadGroupMipMap>,
    pub max_depth: u32,
    pub is_collapsed: bool,
    pub show_thread_roots: bool,
}

#[derive(Debug, Clone)]
pub struct ThreadGroupMipMap {
    pub max_duration_ns: u64,
    pub events: Vec<EventId>,
    pub shadows: ThreadGroupMipMapShadows,
    pub events_by_start: Vec<usize>,
    pub events_by_end: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct ThreadGroupMipMapShadows {
    pub events: Vec<Shadow>,
    pub events_by_start: Vec<usize>,
    pub events_by_end: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Shadow {
    pub start_ns: UnalignedU64,
    pub duration_ns: UnalignedU64,
    pub depth: u32,
}

pub fn thread_group_key(group: &ThreadGroup) -> ThreadGroupKey {
    Arc::as_ptr(&group.threads) as ThreadGroupKey
}

#[derive(Debug, Clone, Default)]
pub struct TimelineData {
    pub thread_groups: Vec<ThreadGroup>,
    pub min_ns: u64,
    pub max_ns: u64,
}

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
    pub color_mode: crate::timeline::ColorMode,
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
            color_mode: crate::timeline::ColorMode::default(),
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
                color: crate::timeline::color_from_label(""),
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
        let color = color_from_hsl(hue, 0.35, 0.8);
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
            .unwrap_or_else(|| color_from_hsl(0.0, 0.0, 0.85));
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
        let (_events, max_depth, mipmaps) = build_thread_group_events(events, &threads, false);
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
            build_thread_group_events(events, &threads, show_thread_roots);
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

pub fn build_thread_group_events(
    events: &mut [TimelineEvent],
    threads: &[Arc<ThreadData>],
    show_thread_roots: bool,
) -> (Vec<EventId>, u32, Vec<ThreadGroupMipMap>) {
    let mut event_ids = Vec::new();
    for thread in threads {
        if show_thread_roots && let Some(root_id) = thread.thread_root {
            event_ids.push(root_id);
        }
        event_ids.extend(thread.events.iter().copied());
    }

    event_ids.sort_by_key(|event_id| {
        let event = &events[event_id.index()];
        (
            event.start_ns,
            event.thread_id,
            display_depth(show_thread_roots, event),
        )
    });

    let max_depth = event_ids
        .iter()
        .map(|event_id| display_depth(show_thread_roots, &events[event_id.index()]))
        .max()
        .unwrap_or(0);
    let mipmaps = build_thread_group_mipmaps(events, &event_ids);
    (event_ids, max_depth, mipmaps)
}

// Small helper used during initial event creation before kind colors are
// computed. We return a neutral grey as a placeholder — the real colors are
// applied later in `apply_kind_colors`.
// Small placeholder color used briefly during initial event collection.
// It is intentionally inlined where necessary; keep the helper removed to
// avoid an unused function warning.

pub(crate) fn event_end_ns(events: &[TimelineEvent], event_id: EventId) -> u64 {
    let event = &events[event_id.index()];
    event.start_ns.saturating_add(event.duration_ns)
}

fn build_event_indices(
    events: &[TimelineEvent],
    event_ids: &[EventId],
) -> (Vec<usize>, Vec<usize>) {
    let mut events_by_start: Vec<usize> = (0..event_ids.len()).collect();
    events_by_start.sort_by_key(|&index| {
        let event = &events[event_ids[index].index()];
        (event.start_ns, event.thread_id, event.depth)
    });

    let mut events_by_end: Vec<usize> = (0..event_ids.len()).collect();
    events_by_end.sort_by_key(|&index| {
        let event_id = event_ids[index];
        let event = &events[event_id.index()];
        (
            event_end_ns(events, event_id),
            event.start_ns,
            event.thread_id,
        )
    });

    (events_by_start, events_by_end)
}

fn duration_bucket(duration_ns: u64) -> u32 {
    let duration = duration_ns.max(1);
    63u32 - duration.leading_zeros()
}

fn build_thread_group_mipmaps(
    events: &mut [TimelineEvent],
    event_ids: &[EventId],
) -> Vec<ThreadGroupMipMap> {
    if event_ids.is_empty() {
        return Vec::new();
    }

    let mut buckets: Vec<Vec<EventId>> = Vec::new();
    for event_id in event_ids {
        let event = &events[event_id.index()];
        let bucket = duration_bucket(event.duration_ns) as usize;
        if buckets.len() <= bucket {
            buckets.resize_with(bucket + 1, Vec::new);
        }
        buckets[bucket].push(*event_id);
    }

    let mut mipmaps = Vec::new();
    for (bucket, bucket_events) in buckets.into_iter().enumerate() {
        if bucket_events.is_empty() {
            continue;
        }
        let (events_by_start, events_by_end) = build_event_indices(events, &bucket_events);
        let max_duration_ns = if bucket >= 63 {
            u64::MAX
        } else {
            (1u64 << (bucket as u32 + 1)).saturating_sub(1)
        };
        mipmaps.push(ThreadGroupMipMap {
            max_duration_ns,
            events: bucket_events,
            shadows: ThreadGroupMipMapShadows::default(),
            events_by_start,
            events_by_end,
        });
    }

    // Add per-level shadow events so very small events remain visible as ~1px
    // markers when zooming out.
    //
    // For each level i (in increasing duration order), build a cumulative shadow
    // representation of all real events in levels [0..=i], inflated to at least
    // that level's max_duration and merged per (thread_id, depth, is_root).
    //
    // Shadows are stored separately per mip level so the main `events` list
    // remains purely "real" events.
    if !mipmaps.is_empty() {
        let mut cumulative_real: Vec<EventId> = Vec::new();
        for level in mipmaps.iter_mut() {
            cumulative_real.extend(level.events.iter().copied());

            let target_min_duration = level.max_duration_ns.max(1);
            let mut intervals: Vec<(u32, u64, u64)> = Vec::with_capacity(cumulative_real.len());
            for &event_id in &cumulative_real {
                let event = &events[event_id.index()];
                let start = event.start_ns;
                let inflated = event.duration_ns.max(target_min_duration);
                let end = start.saturating_add(inflated);
                intervals.push((event.depth, start, end));
            }

            // Sort by depth then start so we can merge overlapping intervals per
            // depth level.
            intervals.sort_by_key(|&(depth, start, _end)| (depth, start));

            let mut merged: Vec<(u32, u64, u64)> = Vec::new();
            for interval in intervals {
                if let Some(last) = merged.last_mut() {
                    let (ldepth, lstart, lend) = *last;
                    let (depth, start, end) = interval;
                    if ldepth == depth && start <= lend {
                        *last = (ldepth, lstart, lend.max(end));
                        continue;
                    }
                }
                merged.push(interval);
            }

            if merged.is_empty() {
                continue;
            }

            let mut shadows: Vec<Shadow> = Vec::with_capacity(merged.len());
            for (depth, start, end) in merged {
                let duration = end.saturating_sub(start).max(1);

                shadows.push(Shadow {
                    start_ns: UnalignedU64::new(start),
                    duration_ns: UnalignedU64::new(duration),
                    depth,
                });
            }

            level.shadows.events = shadows;
            // Build index arrays for shadows by sorting indices by start/end
            let mut indices: Vec<usize> = (0..level.shadows.events.len()).collect();
            indices.sort_by_key(|&i| {
                let s = &level.shadows.events[i];
                (s.start_ns.get(), s.depth)
            });
            level.shadows.events_by_start = indices;

            let mut indices_end: Vec<usize> = (0..level.shadows.events.len()).collect();
            indices_end.sort_by_key(|&i| {
                let s = &level.shadows.events[i];
                (
                    s.start_ns.get().saturating_add(s.duration_ns.get()),
                    s.start_ns.get(),
                    s.depth,
                )
            });
            level.shadows.events_by_end = indices_end;
        }
    }

    mipmaps
}
