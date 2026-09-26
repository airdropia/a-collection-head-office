// v0.35.0: Library crate — Phase B refactor.
// All modules moved here from main.rs so that BOTH binaries can share the
// exact same command/business layer:
//   1. a-collection-head-office.exe  (Tauri GUI app — src/main.rs)
//   2. acollectionho.exe             (write-CLI for agent ops — src/bin/acollectionho.rs)
// Behavior unchanged. main.rs is now a thin wrapper that calls run().

pub mod database;
pub mod catalog;
pub mod catalog_publish;
pub mod inventory;
pub mod customers;
pub mod reports;
pub mod automation;
pub mod utils;
pub mod commands;

use commands::DbState;

pub fn run() {
    let db_path = utils::get_db_path();
    let conn = database::init_db(&db_path).expect("Failed to initialize SQLite database");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // v0.14.5: Clipboard manager for image sharing. Lets the frontend
        // call writeImage(bytes) to put a product image on the system
        // clipboard, so the user can paste it into FB/IG/WhatsApp post
        // composers after we open the share URL.
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(DbState(tauri::async_runtime::Mutex::new(conn)))
        .setup(move |app| {
            let app_handle = app.handle().clone();
            automation::start_scheduler(db_path, app_handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::products_commands::get_products,
            commands::products_commands::add_product,
            commands::products_commands::update_product,
            commands::products_commands::delete_product,
            commands::products_commands::export_products_csv,
            commands::products_commands::import_products_csv,
            commands::products_commands::upload_product_image,
            commands::products_commands::get_image_as_base64,
            commands::products_commands::save_base64_image,
            commands::products_commands::save_image_from_url,
            commands::products_commands::save_image_for_share,
            // v0.33.0 — Manual sold-out marking
            commands::products_commands::mark_product_sold_out,
            commands::backup_commands::list_backups,
            commands::backup_commands::restore_backup,
            commands::backup_commands::import_from_catalog_json,
            commands::inventory_commands::get_inventory_summary,
            commands::inventory_commands::get_low_stock,
            commands::inventory_commands::get_dead_stock,
            commands::inventory_commands::get_best_sellers,
            commands::inventory_commands::adjust_stock,
            commands::customers_commands::get_customers,
            commands::customers_commands::add_customer,
            commands::customers_commands::update_customer,
            commands::customers_commands::delete_customer,
//            create_order/get_customer_history REMOVED v0.38.0 (legacy cart
//            path — decremented stock WITHOUT a sales row; root cause of
//            stale stock data. record_sale is the only sales path now.)
            // v0.35.0 — Phase B: balance recompute (auto-heal support)
            commands::customers_commands::recompute_customer_balances,
            commands::reports_commands::get_sales_report,
            commands::reports_commands::get_inventory_report,
            commands::reports_commands::get_customer_report,
            commands::settings_commands::get_settings,
            commands::settings_commands::update_setting,
            commands::backup_commands::backup_database_now,
            commands::backup_commands::init_database,
            // v0.11.1 — Share Center: REMOVED in v0.38.0 (pi audit: share_logs
            // 0 rows EVER; owner verdict — dead feature)
            // v0.11.2 — Purchase Trips: REMOVED in v0.38.0 (pi audit: 2 header-only
            // trips, 0 items ever; owner verdict — dead feature)
            // v0.12.5 — Sales
            commands::sales_commands::record_sale,
            // v0.30.0 — Sale undo + sold items reactivation
            commands::sales_commands::undo_sale,
            commands::sales_commands::reactivate_sold_product,
            // v0.39.0 — Dashboard Recent Sales panel (pi suggestion #3)
            commands::sales_commands::get_recent_sales,
            // v0.26.0 — Customer Udhar/Credit (khata)
            commands::udhar_commands::record_customer_payment,
            commands::udhar_commands::get_customer_balance_history,
            // v0.29.0 — Customer manual ledger entries (opening balance + adjustments)
            commands::udhar_commands::add_customer_ledger_entry,
            commands::udhar_commands::update_customer_ledger_entry,
            commands::udhar_commands::delete_customer_ledger_entry,
            // v0.15.0 — Public Catalog Publishing
            commands::catalog_publish_commands::preview_catalog_publish,
            commands::catalog_publish_commands::publish_catalog_to_github,
            commands::catalog_publish_commands::get_catalog_publish_history,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
