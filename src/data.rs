use crate::timeline::{self, ThreadData, ThreadGroup, TimelineData, TimelineEvent};
use analyzeme::ProfilingData;
use iced::Color;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Stats {
    pub event_count: usize,
    pub cmd: String,
    pub pid: u32,
    pub timeline: TimelineData,
    pub merged_thread_groups: Vec<ThreadGroup>,
    // UI/state fields that are only meaningful once the file is loaded.
    pub color_mode: timeline::ColorMode,
    pub selected_event: Option<TimelineEvent>,
    pub hovered_event: Option<TimelineEvent>,
    pub merge_threads: bool,
    pub initial_fit_done: bool,
    pub view_type: crate::ViewType,
    pub zoom_level: f32,
    pub scroll_offset: iced::Vector,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub load_duration_ns: Option<u64>,
}

pub fn load_profiling_data(path: &Path) -> Result<Stats, String> {
    let stem = path.with_extension("");

    let data = ProfilingData::new(&stem)
        .map_err(|e| format!("Failed to load profiling data from {:?}: {}", stem, e))?;

    let metadata = data.metadata();

    // First gather all parsed events into a temporary buffer so we can
    // determine the distinct event kinds and assign hues evenly across them.
    let mut parsed_events: Vec<(u64, String, u64, u64, String, Vec<String>, Option<u64>)> =
        Vec::new();
    let mut min_ns = u64::MAX;
    let mut max_ns = 0;
    let mut event_count = 0;

    for lightweight_event in data.iter() {
        let event = data.to_full_event(&lightweight_event);
        let thread_id = event.thread_id as u64;

        if let analyzeme::EventPayload::Timestamp(timestamp) = &event.payload {
            if let analyzeme::Timestamp::Interval { start, end } = timestamp {
                event_count += 1;

                let start_ns = start
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;
                let end_ns = end
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;

                min_ns = min_ns.min(start_ns);
                max_ns = max_ns.max(end_ns);

                let event_kind = event.event_kind.to_string();
                let additional_data = event
                    .additional_data
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                let payload_integer = event.payload.integer();

                parsed_events.push((
                    thread_id,
                    event.label.to_string(),
                    start_ns,
                    end_ns.saturating_sub(start_ns),
                    event_kind,
                    additional_data,
                    payload_integer,
                ));
            }
        }
    }

    // Build a deterministic ordered list of kinds and assign equally spaced hues.
    use std::collections::BTreeSet;
    let mut kinds_set: BTreeSet<String> = BTreeSet::new();
    for (_, _, _, _, kind, _, _) in &parsed_events {
        kinds_set.insert(kind.clone());
    }
    let kinds: Vec<String> = kinds_set.into_iter().collect();
    let kind_count = kinds.len().max(1);

    let mut kind_color_map: HashMap<String, Color> = HashMap::new();
    // Start the hue at green (~120°) so the first kind maps to green, then
    // evenly step around the wheel.
    let base_hue = 120.0_f32;
    for (i, kind) in kinds.iter().enumerate() {
        let step = 360.0 / kind_count as f32;
        let hue = (base_hue + (i as f32) * step) % 360.0;
        // Adjust saturation/lightness to better match the previous
        // hash-derived palette (muted, bright colors concentrated in the
        // upper RGB range). These values aim to reproduce that look.
        let color = timeline::color_from_hsl(hue, 0.35, 0.8);
        kind_color_map.insert(kind.clone(), color);
    }

    // Now assign TimelineEvent entries into threads using the computed colors.
    let mut threads: HashMap<u64, Vec<TimelineEvent>> = HashMap::new();
    for (thread_id, label, start_ns, duration_ns, event_kind, additional_data, payload_integer) in
        parsed_events
    {
        let color = kind_color_map
            .get(&event_kind)
            .cloned()
            .unwrap_or_else(|| timeline::color_from_hsl(0.0, 0.0, 0.85));

        threads.entry(thread_id).or_default().push(TimelineEvent {
            label,
            start_ns,
            duration_ns,
            depth: 0,
            thread_id,
            event_kind,
            additional_data,
            payload_integer,
            color,
            is_thread_root: false,
        });
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
        thread_data_vec.push(Arc::new(ThreadData { thread_id, events }));
    }

    thread_data_vec.sort_by_key(|t| t.thread_id);

    let mut thread_groups = Vec::new();
    for thread in &thread_data_vec {
        let threads = Arc::new(vec![thread.clone()]);
        let (_events, max_depth, mipmaps) = timeline::build_thread_group_events(&threads);
        thread_groups.push(ThreadGroup {
            threads,
            mipmaps,
            max_depth,
            is_collapsed: false,
        });
    }

    let merged_thread_groups = build_merged_thread_groups(&thread_data_vec);

    Ok(Stats {
        event_count,
        cmd: metadata.cmd.clone(),
        pid: metadata.process_id,
        timeline: TimelineData {
            thread_groups,
            min_ns: if min_ns == u64::MAX { 0 } else { min_ns },
            max_ns,
        },
        merged_thread_groups,
        color_mode: timeline::ColorMode::default(),
        selected_event: None,
        hovered_event: None,
        merge_threads: false,
        initial_fit_done: false,
        view_type: crate::ViewType::default(),
        zoom_level: 1.0,
        scroll_offset: iced::Vector::default(),
        viewport_width: 0.0,
        viewport_height: 0.0,
        load_duration_ns: None,
    })
}

fn build_merged_thread_groups(threads: &[Arc<ThreadData>]) -> Vec<ThreadGroup> {
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
            for event in &thread.events {
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
        let (_events, max_depth, mipmaps) = timeline::build_thread_group_events(&threads);
        thread_groups.push(ThreadGroup {
            threads,
            mipmaps,
            max_depth,
            is_collapsed: false,
        });
    }

    thread_groups
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
