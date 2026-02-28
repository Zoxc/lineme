# Agent Information for Lineme

This document provides context and guidelines for AI agents working on the `lineme` codebase.

## Purpose
`lineme` is a GUI application built in Rust for viewing and analyzing `measureme` profiling data (used by `rustc` self-profiling). It allows users to open multiple `.mm_profdata` files in tabs and view relevant statistics.

## Tech Stack
- **Language:** Rust (Edition 2024)
- **GUI Framework:** [iced](https://github.com/iced-rs/iced) (v0.14)
- **UI Components:** [iced_aw](https://github.com/iced-rs/iced_aw) (Tabs widget)
- **Profiling Data:** [analyzeme](https://github.com/rust-lang/measureme) (Standard reader for `measureme` files)
- **File Dialogs:** [rfd](https://github.com/PolyFrog/rfd) (Async native dialogs)

## Architecture
- **State Management:** Follows the Elm Architecture (TEA) via `iced::application`.
- **Main Entry:** `src/main.rs` contains the `Lineme` struct which holds the application state (open files and settings).
- **Messages:** The `Message` enum defines all possible interactions (opening files, switching tabs, etc.).
- **Async Operations:** File loading and dialogs are handled via `iced::Task`.

## File Overview
- `src/main.rs`: Main application entry point, state management (Elm Architecture), and tab management.
- `src/scrollbar.rs`: Custom canvas-based scrollbar component (unconnected, f64-based).
- `src/timeline.rs`: Core timeline visualization logic, including event details and coordinate mapping.
- `src/timeline/header.rs`: Renders the time axis and markers at the top of the timeline.
- `src/timeline/threads.rs`: Handles thread label rendering and interaction (collapse/expand).
- `src/timeline/mini_timeline.rs`: Implements the high-level overview for quick navigation and zooming.
- `src/settings.rs`: Encapsulates the settings view and logic (register file extension, hint list).
- `src/file.rs`: Manages tab state helpers and helpers delegating to `data::ProfileData`.
- `src/data.rs`: Data loading and model definitions (`ProfileData`, `TimelineEvent`, `ThreadGroup`, mipmaps).
- `src/tooltip.rs`: Custom tooltip overlay widget for hover information display.
- `src/symbols.rs`: String interner for deduplicating event labels and kind names.
- `src/ui.rs`: Small UI helper utilities.
- `src/timeline/events.rs`: Canvas program for drawing timeline events, handling hover/selection and scrolling.
- `src/timeline/ticks.rs`: Shared helpers for computing nice time tick intervals and formatting labels.

## Development Guidelines for Agents
- **Update File Overview:** If you create, rename, or significantly change the responsibility of a file, update the "File Overview" section above.
- **Dependency Management:** Always check `Cargo.toml` before assuming a crate is available. Note that `analyzeme` is used for reading files; `measureme` is a transitive dependency via `analyzeme`.
- **Iced API:** We use the `application(...)` builder pattern introduced in later `iced` versions. Avoid the older `Application` trait implementation if possible.
- **Lifetimes:** When defining `view` or sub-view functions, explicitly use `Element<'_, Message>` to handle elided lifetimes correctly in `iced`.
- **Error Handling:** Use `Message::ErrorOccurred(String)` to propagate errors to the UI.
- A type is not a type alias.
- Don't leave comments after removing or moving something for historical context.
- Don't fix warnings by ignoring them, adding `allow` attributes or adding `_` to names.
- Don't use `git`.
- Run `cargo check` after making changes and fix any warnings and errors.
- Preserve comments when making edits

## Timeline Features
- **Visualization:** Multi-threaded timeline view using a custom `iced` Canvas program.
    - **Optimization:** Skip drawing and interaction for events smaller than 5 pixels (calculated using zoom level) to improve performance.
    - **Mipmaps:** Thread groups precompute log2 duration buckets with per-level event lists and sorted indices; levels too small to reach 1px are skipped when zoomed out.
- **Mini Timeline:** Always-visible overview above the main timeline. It does not scroll or zoom and shows the full timeline range.
    - **Viewport Indicator:** The current main timeline view is highlighted on the mini timeline.
    - **Navigation:** Left click to pan the main timeline to the clicked position.
    - **Selection Zoom:** Right click and drag to select a range; the main timeline pans and zooms to match the selection.
- **Zooming:** Use the mouse wheel to zoom horizontally. Zoom is centered on the mouse position.
    - **Events Area:** Mouse-wheel zoom centers on the cursor within the events viewport.
- **Scrolling:** Use Ctrl + mouse wheel to scroll vertically. Horizontal and vertical scrolling are also available via scrollbars.
- **Event Selection:** Click on an event to select it. Selection is highlighted and details are shown in a dedicated panel.
- **Thread Management:** 
    - Click thread labels to toggle collapse/expand.
    - **Collapsed Mode:** Only shows topmost (depth 0) events for a compact overview while hiding nested details.
- **Sticky Elements:** Thread labels and time markers remain visible while scrolling.
