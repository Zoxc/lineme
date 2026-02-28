#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnalignedU64([u8; 8]);

impl UnalignedU64 {
    pub fn new(v: u64) -> Self {
        Self(v.to_le_bytes())
    }

    pub fn get(&self) -> u64 {
        u64::from_le_bytes(self.0)
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
use intervaltree::IntervalTree;
use rayon::prelude::*;
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
    // Index into FileData.kinds identifying the kind/color for this event.
    // Stored as u16 to keep per-event memory small.
    pub kind_index: u16,
    pub additional_data: Option<Box<[crate::symbols::Symbol]>>,
    pub payload_integer: Option<u64>,
    pub is_thread_root: bool,
}

#[derive(Debug, Clone)]
pub struct ThreadData {
    pub thread_id: u32,
    pub thread_root: Option<EventId>,
    pub thread_root_mipmap: Option<ThreadGroupMipMap>,
    pub mipmaps: Vec<ThreadGroupMipMap>,
    pub max_depth: u32,
}

// Event-related types are defined below in this file.
pub type ThreadGroupId = Arc<Vec<Arc<ThreadData>>>;
pub type ThreadGroupKey = usize;

#[derive(Debug, Clone)]
pub struct ThreadGroup {
    pub threads: ThreadGroupId,
    pub max_depth: u32,
    pub is_collapsed: bool,
    pub show_thread_roots: bool,
}

#[derive(Debug, Clone)]
pub struct ThreadGroupMipMap {
    pub max_duration_ns: u64,
    pub events: Vec<EventId>,
    pub shadows: ThreadGroupMipMapShadows,
    pub events_tree: IntervalTree<u64, EventId>,
}

#[derive(Debug, Clone, Default)]
pub struct ThreadGroupMipMapShadows {
    // One level per depth, each containing shadows at that depth and their tree.
    pub levels: Vec<ShadowLevel>,
}

/// All shadows at a single depth level, stored in an interval tree.
/// The tree stores the time range as the key and () as the value.
#[derive(Debug, Clone)]
pub struct ShadowLevel {
    pub events_tree: IntervalTree<u64, ()>,
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
    // Compact table of distinct event kinds with their assigned colors.
    pub kinds: Vec<KindInfo>,
    // Simple symbol interner for event strings so we store compact symbol ids
    // in events rather than repeated Strings.
    pub symbols: crate::symbols::Symbols,
}

#[derive(Debug, Clone, Copy)]
pub struct KindInfo {
    pub kind: crate::symbols::Symbol,
    pub color: Color,
}

#[derive(Debug, Clone)]
pub struct FileUi {
    pub color_mode: crate::timeline::ColorMode,
    pub selected_event: Option<EventId>,
    pub hovered_event: Option<EventId>,
    pub hovered_event_position: Option<iced::Point>,
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
            hovered_event_position: None,
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
    // Build compact kinds table for mapping event kinds -> colors. Thread-root
    // events are created later and use a fixed color instead of the kind table.
    let (kinds, kind_map) = build_kind_table(&collected.event_kinds, &symbols);
    // Ensure the kinds table fits in a u16 index stored per-event.
    if kinds.len() > (u16::MAX as usize) {
        return Err(format!(
            "Too many distinct event kinds: {} (max {})",
            kinds.len(),
            u16::MAX
        ));
    }
    let mut events = collected.events;
    // Assign per-event kind indices from the precomputed kind map using the
    // parallel `collected.event_kinds` array recorded during parsing.
    events.par_iter_mut().enumerate().for_each(|(i, event)| {
        if let Some(kind_sym) = collected.event_kinds.get(i).copied()
            && let Some(&idx) = kind_map.get(&kind_sym)
        {
            event.kind_index = idx as u16;
            return;
        }
        // Fallback to first kind (shouldn't happen since map built from events)
        event.kind_index = 0u16;
    });
    let mut threads = build_threads_index(&events);
    assign_event_depths(&mut events, &mut threads);
    let thread_data_vec = build_thread_data(&mut events, threads, &mut symbols);
    let thread_groups = build_thread_groups(&thread_data_vec);
    let merged_thread_groups = build_merged_thread_groups(&events, &thread_data_vec);

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
            // store the precomputed kinds table for render-time lookup
            kinds,
            symbols,
        },
        ui: FileUi::default(),
        load_duration_ns: None,
    })
}

#[derive(Debug)]
struct CollectedEvents {
    events: Vec<TimelineEvent>,
    /// Original event kind symbols (one per event) used to build the kinds table
    /// before we drop per-event kind storage. The order matches `events`.
    event_kinds: Vec<crate::symbols::Symbol>,
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
    // Use ProfilingData::num_events() as a fast count for pre-allocation and
    // for reporting event_count. This avoids walking the iterator twice.
    let event_count: usize = data.num_events();
    let mut events = Vec::with_capacity(event_count);
    let mut max_ns: u64 = 0;
    let mut event_kinds: Vec<crate::symbols::Symbol> = Vec::with_capacity(event_count);

    for lightweight_event in data.iter() {
        let event = data.to_full_event(&lightweight_event);
        let thread_id = event.thread_id;

        if let analyzeme::EventPayload::Timestamp(analyzeme::Timestamp::Interval { start, end }) =
            &event.payload
        {
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

            // Collect additional_data into an optional boxed slice to avoid
            // allocating for the common case where there is no additional
            // data. Convert to `None` when empty to save the boxed allocation.
            let mut additional_data_vec = Vec::with_capacity(event.additional_data.len());
            for s in &event.additional_data {
                additional_data_vec.push(symbols.intern(s.as_ref()));
            }
            let additional_data = if additional_data_vec.is_empty() {
                None
            } else {
                Some(additional_data_vec.into_boxed_slice())
            };

            // Record the original event kind symbol for later table-building
            event_kinds.push(symbols.intern(event.event_kind.as_ref()));

            events.push(TimelineEvent {
                thread_id,
                label: symbols.intern(event.label.as_ref()),
                start_ns,
                duration_ns: end_ns.saturating_sub(start_ns),
                depth: 0,
                kind_index: 0u16,
                additional_data,
                payload_integer: event.payload.integer(),
                // No per-event color stored any more; colors are looked up from
                // `FileData::kind_color_map` at render time.
                is_thread_root: false,
            });
        }
    }

    events.shrink_to_fit();

    CollectedEvents {
        events,
        event_kinds,
        max_ns,
        event_count,
    }
}

// Legacy helper retained for compatibility with older code paths. New code
// uses `build_kind_table` which produces a Vec<KindInfo> and a map.

// Build a compact Vec of distinct kinds with assigned colors and a map from
// kind Symbol -> index in that Vec. Returned Vec order is deterministic.
fn build_kind_table(
    event_kinds: &[crate::symbols::Symbol],
    symbols: &crate::symbols::Symbols,
) -> (Vec<KindInfo>, HashMap<crate::symbols::Symbol, usize>) {
    let mut kinds_set: std::collections::HashSet<crate::symbols::Symbol> =
        event_kinds.iter().copied().collect();
    let mut kinds: Vec<crate::symbols::Symbol> = kinds_set.drain().collect();
    kinds.sort_by_key(|s| symbols.resolve(*s));

    let kind_count = kinds.len().max(1);
    let base_hue = 120.0_f32;

    let mut vec: Vec<KindInfo> = Vec::with_capacity(kind_count);
    let mut map: HashMap<crate::symbols::Symbol, usize> = HashMap::new();
    for (i, &kind_sym) in kinds.iter().enumerate() {
        let step = 360.0 / kind_count as f32;
        let hue = (base_hue + (i as f32) * step) % 360.0;
        let color = color_from_hsl(hue, 0.35, 0.8);
        vec.push(KindInfo {
            kind: kind_sym,
            color,
        });
        map.insert(kind_sym, i);
    }

    (vec, map)
}

// `apply_kind_colors` removed: we no longer store per-event colors. Keep no
// dead-code helpers around.

fn build_threads_index(events: &[TimelineEvent]) -> HashMap<u32, Vec<EventId>> {
    let mut threads: HashMap<u32, Vec<EventId>> = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        threads
            .entry(event.thread_id)
            .or_default()
            .push(EventId(index as u32));
    }
    for vec in threads.values_mut() {
        vec.shrink_to_fit();
    }
    threads
}

fn assign_event_depths(events: &mut [TimelineEvent], threads: &mut HashMap<u32, Vec<EventId>>) {
    for thread_events in threads.values_mut() {
        // Sort primarily by start time. For events that share the same start,
        // sort longer events first so the simple stack-based nesting algorithm
        // assigns parents before children.
        thread_events.sort_by(|a, b| {
            let a_event = &events[a.index()];
            let b_event = &events[b.index()];

            match a_event.start_ns.cmp(&b_event.start_ns) {
                std::cmp::Ordering::Equal => {
                    let a_end = a_event.start_ns.saturating_add(a_event.duration_ns);
                    let b_end = b_event.start_ns.saturating_add(b_event.duration_ns);
                    b_end.cmp(&a_end)
                }
                other => other,
            }
        });
        let mut stack: Vec<u64> = Vec::new();
        for event_id in thread_events.iter() {
            let event = &mut events[event_id.index()];
            let end_ns = event.start_ns.saturating_add(event.duration_ns);
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

    // Phase 1: Compute thread root info in parallel (immutable access to events)
    let mut thread_root_infos: Vec<(u32, Option<ThreadRootInfo>)> = threads
        .par_iter()
        .map(|(&thread_id, event_ids)| {
            let info = compute_thread_root_info(events, thread_id, event_ids);
            (thread_id, info)
        })
        .collect();

    // Keep event ids stable across runs by building thread-root events in a
    // deterministic order.
    thread_root_infos.sort_by_key(|(thread_id, _)| *thread_id);

    // Phase 2: Build thread root events and add them to events sequentially
    // (requires mutable access to events and symbols)
    let mut thread_roots: Vec<(u32, Option<EventId>)> = Vec::with_capacity(threads.len());
    for (thread_id, info) in thread_root_infos {
        let event_id = info.map(|root_info| {
            let event = build_thread_root_event(&root_info, symbols);
            let event_id = EventId(events.len() as u32);
            events.push(event);
            event_id
        });
        thread_roots.push((thread_id, event_id));
    }

    // Phase 3: Build thread data in parallel using immutable references to events
    // and pre-computed thread roots
    let threads_for_parallel: Vec<(u32, Vec<EventId>, Option<EventId>)> = thread_roots
        .into_iter()
        .map(|(thread_id, thread_root)| {
            let event_ids = threads.get(&thread_id).cloned().unwrap_or_default();
            (thread_id, event_ids, thread_root)
        })
        .collect();

    type ThreadDataPart = (
        u32,
        Option<EventId>,
        u32,
        Vec<ThreadGroupMipMap>,
        Option<ThreadGroupMipMap>,
    );

    let thread_data_parts: Vec<ThreadDataPart> = threads_for_parallel
        .par_iter()
        .map(|(thread_id, event_ids, thread_root)| {
            // Build thread root mipmap (immutable access to events)
            let thread_root_mipmap = thread_root.map(|root_id| {
                let bucket = duration_bucket(events[root_id.index()].duration_ns) as usize;
                let max_duration_ns = if bucket >= 63 {
                    u64::MAX
                } else {
                    (1u64 << (bucket as u32 + 1)).saturating_sub(1)
                };
                let bucket_events = vec![root_id];
                let (_events_by_start, _events_by_end, events_tree) =
                    build_event_indices(events, &bucket_events);
                ThreadGroupMipMap {
                    max_duration_ns,
                    events: bucket_events,
                    shadows: ThreadGroupMipMapShadows::default(),
                    events_tree,
                }
            });

            // Calculate max depth (immutable access to events)
            let max_depth = event_ids
                .iter()
                .map(|event_id| events[event_id.index()].depth)
                .max()
                .unwrap_or(0);

            // Build mipmaps for this thread (immutable access to events)
            let mipmaps = build_thread_group_mipmaps(events, event_ids);

            (
                *thread_id,
                *thread_root,
                max_depth,
                mipmaps,
                thread_root_mipmap,
            )
        })
        .collect();

    // Phase 4: Construct final ThreadData objects
    for (thread_id, thread_root, max_depth, mipmaps, thread_root_mipmap) in thread_data_parts {
        thread_data_vec.push(Arc::new(ThreadData {
            thread_id,
            thread_root,
            thread_root_mipmap,
            mipmaps,
            max_depth,
        }));
    }

    thread_data_vec.sort_by_key(|t| t.thread_id);
    thread_data_vec
}

fn build_thread_groups(thread_data: &[Arc<ThreadData>]) -> Vec<ThreadGroup> {
    let mut thread_groups = Vec::new();
    for thread in thread_data {
        let threads = Arc::new(vec![thread.clone()]);
        thread_groups.push(ThreadGroup {
            threads,
            max_depth: thread.max_depth,
            is_collapsed: false,
            show_thread_roots: false,
        });
    }

    thread_groups
}

fn build_merged_thread_groups(
    events: &[TimelineEvent],
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
            if let Some(root) = thread.thread_root {
                let event = &events[root.index()];
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

        let max_depth = threads
            .iter()
            .map(|t| t.max_depth)
            .max()
            .unwrap_or(0)
            .saturating_add(if show_thread_roots { 1 } else { 0 });
        thread_groups.push(ThreadGroup {
            threads,
            max_depth,
            is_collapsed: false,
            show_thread_roots,
        });
    }

    thread_groups
}

/// Information needed to build a thread root event.
#[derive(Debug)]
struct ThreadRootInfo {
    thread_id: u32,
    start_ns: u64,
    duration_ns: u64,
}

fn compute_thread_root_info(
    events: &[TimelineEvent],
    thread_id: u32,
    event_ids: &[EventId],
) -> Option<ThreadRootInfo> {
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

    Some(ThreadRootInfo {
        thread_id,
        start_ns,
        duration_ns: end_ns.saturating_sub(start_ns),
    })
}

fn build_thread_root_event(
    info: &ThreadRootInfo,
    symbols: &mut crate::symbols::Symbols,
) -> TimelineEvent {
    TimelineEvent {
        label: symbols.intern(&format!("Thread {}", info.thread_id)),
        start_ns: info.start_ns,
        duration_ns: info.duration_ns,
        depth: 0,
        thread_id: info.thread_id,
        kind_index: 0u16,
        additional_data: None,
        payload_integer: None,
        is_thread_root: true,
    }
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

// Small helper used during initial event creation before kind colors are
// computed. We return a neutral grey as a placeholder — the real colors are
// applied later in `apply_kind_colors`.
// Small placeholder color used briefly during initial event collection.
// It is intentionally inlined where necessary; keep the helper removed to
// avoid an unused function warning.

// helper `event_end_ns` was removed in favor of using event fields directly.

fn build_event_indices(
    events: &[TimelineEvent],
    event_ids: &[EventId],
) -> (Vec<usize>, Vec<usize>, IntervalTree<u64, EventId>) {
    // We no longer maintain start/end index arrays; the interval tree
    // provides fast queries for overlapping intervals.

    // Build an interval tree mapping each event interval to its index in
    // `event_ids` so callers can quickly query overlapping intervals.
    let iter = event_ids.iter().map(|&event_id| {
        let event = &events[event_id.index()];
        let start = event.start_ns;
        let duration = event.duration_ns.max(1);
        (start..start.saturating_add(duration), event_id)
    });
    let events_tree = IntervalTree::from_iter(iter);

    (Vec::new(), Vec::new(), events_tree)
}

fn duration_bucket(duration_ns: u64) -> u32 {
    let duration = duration_ns.max(1);
    63u32 - duration.leading_zeros()
}

fn build_thread_group_mipmaps(
    events: &[TimelineEvent],
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
        let max_duration_ns = if bucket >= 63 {
            u64::MAX
        } else {
            (1u64 << (bucket as u32 + 1)).saturating_sub(1)
        };
        if bucket_events.is_empty() {
            // Keep empty levels so the shadow system has intermediate
            // levels between populated buckets.  Without these the
            // smallest-visible-level selection can jump across a large
            // gap, causing shadows to be inflated far beyond ~1 px.
            mipmaps.push(ThreadGroupMipMap {
                max_duration_ns,
                events: Vec::new(),
                shadows: ThreadGroupMipMapShadows::default(),
                events_tree: IntervalTree::from_iter(std::iter::empty::<(std::ops::Range<u64>, EventId)>()),
            });
            continue;
        }
        let (_events_by_start, _events_by_end, events_tree) =
            build_event_indices(events, &bucket_events);
        mipmaps.push(ThreadGroupMipMap {
            max_duration_ns,
            events: bucket_events,
            shadows: ThreadGroupMipMapShadows::default(),
            events_tree,
        });
    }

    // Add per-level shadow events so very small events remain visible as ~1px
    // markers when zooming out.
    //
    // For each level i (in increasing duration order), build a cumulative shadow
    // representation of all real events in levels [0..i), inflated to at least
    // that level's max_duration and merged per depth level.
    //
    // Build this incrementally: for each level, first re-inflate the accumulated
    // shadows from previous levels to the new minimum duration, store those as
    // the current level's shadows, then merge in the current level's real events
    // so the next level sees them.
    //
    // Shadows are stored separately per mip level so the main `events` list
    // remains purely "real" events.
    if !mipmaps.is_empty() {
        fn push_merged(out: &mut Vec<(u64, u64)>, start: u64, end: u64) {
            if let Some(last) = out.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
                return;
            }
            out.push((start, end));
        }

        // Cumulative merged shadow ranges per depth for *previous* levels,
        // sorted by start and non-overlapping.
        let mut merged_by_depth: Vec<Vec<(u64, u64)>> = Vec::new();

        for level in mipmaps.iter_mut() {
            let target_min_duration = level.max_duration_ns.max(1);

            // Collect the current level's real events by depth, inflated to the
            // target min duration.
            let mut new_by_depth: Vec<Vec<(u64, u64)>> = Vec::new();
            for &event_id in &level.events {
                let event = &events[event_id.index()];
                let start = event.start_ns;
                let inflated = event.duration_ns.max(target_min_duration).max(1);
                let mut end = start.saturating_add(inflated);
                end = end.max(start.saturating_add(1));

                let depth = event.depth as usize;
                if new_by_depth.len() <= depth {
                    new_by_depth.resize_with(depth + 1, Vec::new);
                }
                new_by_depth[depth].push((start, end));
            }

            let depth_count = merged_by_depth.len().max(new_by_depth.len());
            if merged_by_depth.len() < depth_count {
                merged_by_depth.resize_with(depth_count, Vec::new);
            }
            if new_by_depth.len() < depth_count {
                new_by_depth.resize_with(depth_count, Vec::new);
            }

            // Re-inflate the previous shadows to the new min duration and merge
            // any overlaps introduced by the increased duration.
            let mut reinflated_by_depth: Vec<Vec<(u64, u64)>> = Vec::with_capacity(depth_count);
            for depth in 0..depth_count {
                let old = std::mem::take(&mut merged_by_depth[depth]);
                let mut reinflated: Vec<(u64, u64)> = Vec::with_capacity(old.len());
                for (start, end) in old {
                    let duration = end.saturating_sub(start).max(1);
                    let inflated = duration.max(target_min_duration).max(1);
                    let mut new_end = start.saturating_add(inflated);
                    new_end = new_end.max(start.saturating_add(1));
                    push_merged(&mut reinflated, start, new_end);
                }
                reinflated_by_depth.push(reinflated);
            }

            // Store shadows for this level as the cumulative result of previous
            // levels only. This avoids drawing a shadow for events that are
            // already visible in this mip level.
            level.shadows.levels = reinflated_by_depth
                .iter()
                .map(|ranges| {
                    let intervals: Vec<_> = ranges
                        .iter()
                        .map(|&(start, end)| (start..end, ()))
                        .collect();
                    ShadowLevel {
                        events_tree: IntervalTree::from_iter(intervals),
                    }
                })
                .collect();

            // Merge the reinflated previous shadows with the current level's real
            // events, producing the cumulative state for the next level.
            for depth in 0..depth_count {
                let mut new = std::mem::take(&mut new_by_depth[depth]);
                new.sort_by_key(|&(start, _end)| start);

                let reinflated = &reinflated_by_depth[depth];
                let mut merged: Vec<(u64, u64)> = Vec::with_capacity(reinflated.len() + new.len());
                let mut i = 0;
                let mut j = 0;
                while i < reinflated.len() || j < new.len() {
                    let (start, end) = if j >= new.len()
                        || (i < reinflated.len() && reinflated[i].0 <= new[j].0)
                    {
                        let v = reinflated[i];
                        i += 1;
                        v
                    } else {
                        let v = new[j];
                        j += 1;
                        v
                    };
                    push_merged(&mut merged, start, end);
                }

                merged_by_depth[depth] = merged;
            }
        }
    }

    mipmaps
}
