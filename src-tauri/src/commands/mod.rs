//! commands module — thin orchestrator.
//!
//! v0.27.0 refactor: Per-feature commands live in their own files.
//! This file keeps DbState + shared helpers + re-exports for backward
//! compatibility so `commands::get_products` etc. continue to work
//! without touching main.rs invoke_handler.
//!
//! See worklog.md Task ID: refactor-1.

use tauri::async_runtime::Mutex;
use rusqlite::Connection;

// Per-feature command modules
pub mod products_commands;
pub mod inventory_commands;
pub mod customers_commands;
pub mod reports_commands;
pub mod settings_commands;
pub mod backup_commands;
pub mod share_segments_commands;
pub mod purchase_trips_commands;
pub mod sales_commands;
pub mod udhar_commands;
pub mod catalog_publish_commands;

// ============================================================
// Shared state + helpers (used by all command modules)
// ============================================================

/// Database state shared across all Tauri commands.
///
/// Uses `tauri::async_runtime::Mutex` (which is `tokio::sync::Mutex` under the
/// hood) instead of `std::sync::Mutex`. This is critical because:
///
/// 1. **No deadlock across `.await`** — `std::sync::Mutex` is not `Send` when
///    held across `.await` points, which would fail to compile under Tauri's
///    async command model. `tokio::sync::Mutex` is `Send` and safe to hold
///    across awaits.
///
/// 2. **No runtime blocking** — When a command needs to await (e.g., a 45s
///    Gemini API call), other commands can still acquire the lock if needed
///    (though in practice they shouldn't — see pattern below).
///
/// 3. **Pattern discipline** — Even with an async mutex, the codebase follows
///    the scoped-block pattern: acquire lock only for the duration of the
///    synchronous DB operation, then release before any `.await`. This means
///    long AI calls do NOT hold the DB lock, preventing UI freezes.
///
/// Usage:
/// ```ignore
/// let conn = state.0.lock().await;
/// // do synchronous rusqlite work here
/// // drop(conn) — implicit when block ends
/// // .await calls happen AFTER the lock is released
/// ```
pub struct DbState(pub Mutex<Connection>);

pub fn set_setting_val(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2);", [key, value])?;
    Ok(())
}

pub fn get_setting_val(conn: &Connection, key: &str) -> Result<String, rusqlite::Error> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
}

// ============================================================
// Re-exports — keeps `commands::get_products` (etc.) working
// ============================================================

pub use products_commands::{
    get_products, add_product, update_product, delete_product,
    export_products_csv, import_products_csv, upload_product_image,
    get_image_as_base64, save_base64_image, save_image_from_url,
    save_image_for_share, save_drafts_to_folder_with_path,
    mark_product_sold_out,
};
pub use inventory_commands::{
    get_inventory_summary, get_low_stock, get_dead_stock, get_best_sellers, adjust_stock,
};
pub use customers_commands::{
    get_customers, add_customer, update_customer, delete_customer,
    create_order, get_customer_history,
};
pub use reports_commands::{
    get_sales_report, get_inventory_report, get_customer_report,
};
pub use settings_commands::{get_settings, update_setting};
pub use backup_commands::{
    backup_database_now, list_backups, restore_backup,
    import_from_catalog_json, init_database,
};
pub use share_segments_commands::{
    log_share, get_share_logs,
    get_customers_by_segment, update_customer_segment, get_customer_segments,
    get_stale_products,
};
pub use purchase_trips_commands::{
    get_purchase_trips, get_purchase_trip, create_purchase_trip,
    update_purchase_trip, delete_purchase_trip, add_trip_item, remove_trip_item,
};
pub use sales_commands::{record_sale, undo_sale, reactivate_sold_product};
pub use udhar_commands::{
    record_customer_payment, get_customer_balance_history,
    add_customer_ledger_entry, update_customer_ledger_entry, delete_customer_ledger_entry,
};
pub use catalog_publish_commands::{
    preview_catalog_publish, publish_catalog_to_github, get_catalog_publish_history,
};
