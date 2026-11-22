//! The `ironhelix` program: starts the game (`lib.rs`).

// Release builds on Windows open no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    helix_app::main();
}
