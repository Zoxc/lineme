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

## Development Guidelines for Agents
- **Dependency Management:** Always check `Cargo.toml` before assuming a crate is available. Note that `analyzeme` is used for reading files, while `measureme` provides low-level definitions.
- **Iced API:** We use the `application(...)` builder pattern introduced in later `iced` versions. Avoid the older `Application` trait implementation if possible.
- **Lifetimes:** When defining `view` or sub-view functions, explicitly use `Element<'_, Message>` to handle elided lifetimes correctly in `iced`.
- **Error Handling:** Use `Message::ErrorOccurred(String)` to propagate errors to the UI.

## Timeline Features
- **Visualization:** Multi-threaded timeline view using a custom `iced` Canvas program.
    - **Optimization:** Skip drawing and interaction for events smaller than 5 pixels (calculated using zoom level) to improve performance.
- **Zooming:** Use the mouse wheel to zoom in and out. Zoom is centered on the mouse position.
- **Scrolling:** Horizontal and vertical scrolling to navigate large profiling traces.
- **Event Selection:** Click on an event to select it. Selection is highlighted and details are shown in a dedicated panel.
- **Thread Management:** 
    - Click thread labels to toggle collapse/expand.
    - **Collapsed Mode:** Only shows topmost (depth 0) events for a compact overview while hiding nested details.
- **Sticky Elements:** Thread labels and time markers remain visible while scrolling.

## Useful Commands
- `cargo check`: Quickly verify code validity.
- `cargo run`: Launch the application (requires a graphical environment).
- `cargo test`: Run unit tests (add them to `src/` or `tests/`).
