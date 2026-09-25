#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// v0.35.0: Thin wrapper — all modules + run() moved to lib.rs so the
// write-CLI binary (src/bin/acollectionho.rs) can reuse the exact same
// business/command layer (single source of truth for sign conventions).
fn main() {
    a_collection_head_office_lib::run();
}
