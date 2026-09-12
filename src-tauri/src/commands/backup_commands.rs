//! Auto-extracted from commands/mod.rs during v0.27.0 refactor.
//! See worklog.md Task ID: refactor-1 for context.
//!
//! Behavior unchanged — only file structure modified.

use crate::catalog::{self, Product};
use crate::inventory::{self, InventorySummary, LowStockItem, DeadStockItem, BestSellerItem};
use crate::customers::{self, Customer, OrderItemInput, OrderHistory};
use crate::reports::{self, SalesReport, InventoryReport, CustomerSummaryReport};
use crate::agents::{self, AgentSummary, AgentLedgerEntry};
use crate::purchase_trips::{self, PurchaseTripSummary};
use crate::adapters::duckduckgo::{self, WebEvidence};
use crate::ai::{self, AiResponse};
use crate::utils;
use crate::commands::{DbState, set_setting_val, get_setting_val};
use tauri::async_runtime::Mutex;
use std::path::Path;
use rusqlite::{Connection, params};
use tauri::State;

// ============================================================
// // Backup/Restore + import_from_catalog + init_database
// ============================================================

#[tauri::command]
pub async fn backup_database_now(state: State<'_, DbState>) -> Result<String, String> {
    let conn = state.0.lock().await;
    let backup_path = get_setting_val(&conn, "backup_path").map_err(|e| e.to_string())?;
    if backup_path.is_empty() { return Err("Backup path is not configured.".to_string()); }
    let backup_dir = Path::new(&backup_path);
    if !backup_dir.exists() { return Err("Backup path does not exist.".to_string()); }
    let db_src = utils::get_db_path();
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let dest = backup_dir.join(format!("manual_backup_{}.db", timestamp));
    std::fs::copy(db_src, &dest).map_err(|e| format!("Failed to copy: {}", e))?;
    Ok(dest.to_string_lossy().to_string())
}

/// v0.22.0: List available backup files (DB + ZIP) from backup_path.
///
/// Returns Vec of (filename, size_bytes, modified_date_iso) sorted newest first.
/// Used by Settings UI to populate "Restore Backup" dropdown.
#[tauri::command]
pub async fn list_backups(state: State<'_, DbState>) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.0.lock().await;
    let backup_path = get_setting_val(&conn, "backup_path").map_err(|e| e.to_string())?;
    if backup_path.is_empty() {
        return Ok(Vec::new());  // No backup path configured
    }
    let backup_dir = Path::new(&backup_path);
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups: Vec<serde_json::Value> = Vec::new();
    let entries = std::fs::read_dir(backup_dir).map_err(|e| format!("Read dir: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Only include .db and .zip files
        if !name.ends_with(".db") && !name.ends_with(".zip") { continue; }
        // Skip weekly_report text files
        if name.contains("weekly_report") { continue; }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = metadata.len();
        let modified = metadata.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                chrono::DateTime::<chrono::Utc>::from_timestamp(d.as_secs() as i64, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        // Skip empty/corrupt DB files (size < 10 KB = likely empty)
        let is_valid = if name.ends_with(".db") { size > 10240 } else { true };

        backups.push(serde_json::json!({
            "name": name,
            "size": size,
            "modified": modified,
            "is_zip": name.ends_with(".zip"),
            "is_valid": is_valid,
        }));
    }

    // Sort by modified date descending (newest first)
    backups.sort_by(|a, b| {
        b["modified"].as_str().unwrap_or("").cmp(a["modified"].as_str().unwrap_or(""))
    });

    Ok(backups)
}

/// v0.22.0: Restore a backup file (DB-only or full ZIP).
///
/// Ali bhai's requirement: "Restore Backup" button in Settings that auto-picks
/// latest valid backup and restores it.
///
/// Behavior:
/// - If filename ends with .db: copy to AppData/database.db (overwrite existing)
/// - If filename ends with .zip: extract database.db + images/ + settings.json
///   from ZIP into AppData
/// - Before overwrite: create a safety backup of current DB (if exists)
///   named `pre_restore_YYYYMMDD_HHMMSS.db`
/// - Returns success message with what was restored
#[tauri::command]
pub async fn restore_backup(
    state: State<'_, DbState>,
    filename: String,
) -> Result<String, String> {
    use std::io::Write;
    let conn = state.0.lock().await;
    let backup_path = get_setting_val(&conn, "backup_path").map_err(|e| e.to_string())?;
    if backup_path.is_empty() {
        return Err("Backup path is not configured.".to_string());
    }
    drop(conn);  // Release DB lock before file operations

    let backup_dir = Path::new(&backup_path);
    let source_path = backup_dir.join(&filename);
    if !source_path.exists() {
        return Err(format!("Backup file not found: {}", filename));
    }

    let db_path = utils::get_db_path();
    let images_dir = utils::get_images_dir();
    let app_dir = utils::get_app_dir();

    // Safety: backup current DB before overwrite (if it exists)
    let safety_timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    if db_path.exists() {
        let safety_path = backup_dir.join(format!("pre_restore_{}.db", safety_timestamp));
        let _ = std::fs::copy(&db_path, &safety_path);
    }

    if filename.ends_with(".zip") {
        // Full ZIP restore: extract DB + images + settings
        // Phase 1: Read all entries from ZIP (synchronous I/O)
        let zip_file = std::fs::File::open(&source_path)
            .map_err(|e| format!("Failed to open zip: {}", e))?;
        let mut archive = zip::ZipArchive::new(zip_file)
            .map_err(|e| format!("Failed to read zip: {}", e))?;

        std::fs::create_dir_all(&app_dir)
            .map_err(|e| format!("Failed to create app dir: {}", e))?;
        std::fs::create_dir_all(&images_dir)
            .map_err(|e| format!("Failed to create images dir: {}", e))?;

        let mut db_restored = false;
        let mut images_restored = 0;
        let mut settings_data: Option<std::collections::HashMap<String, String>> = None;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)
                .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;
            let name = file.name().to_string();

            if name == "database.db" {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut file, &mut bytes)
                    .map_err(|e| format!("Read DB from zip: {}", e))?;
                std::fs::write(&db_path, &bytes)
                    .map_err(|e| format!("Write DB: {}", e))?;
                db_restored = true;
            } else if name.starts_with("images/") {
                let img_name = name.trim_start_matches("images/");
                if !img_name.is_empty() && !img_name.contains('/') {
                    let dest = images_dir.join(img_name);
                    let mut bytes = Vec::new();
                    std::io::Read::read_to_end(&mut file, &mut bytes)
                        .map_err(|e| format!("Read image: {}", e))?;
                    std::fs::write(&dest, &bytes)
                        .map_err(|e| format!("Write image: {}", e))?;
                    images_restored += 1;
                }
            } else if name == "settings.json" {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut file, &mut bytes)
                    .map_err(|e| format!("Read settings: {}", e))?;
                let settings_str = String::from_utf8_lossy(&bytes).to_string();
                let settings: std::collections::HashMap<String, String> =
                    serde_json::from_str(&settings_str)
                        .map_err(|e| format!("Parse settings JSON: {}", e))?;
                settings_data = Some(settings);
            }
        }

        if !db_restored {
            return Err("ZIP did not contain database.db".to_string());
        }

        // Phase 2: Apply settings to DB (separate from ZIP I/O to keep future Send)
        let mut settings_restored = false;
        if let Some(settings) = settings_data {
            let conn = state.0.lock().await;
            for (key, value) in &settings {
                let _ = (&*conn).execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    rusqlite::params![key, value],
                );
            }
            settings_restored = true;
        }

        Ok(format!(
            "Full backup restored successfully!\n\nDatabase: ✓\nImages: {} restored\nSettings: {}",
            images_restored,
            if settings_restored { "✓" } else { "✗" }
        ))
    } else if filename.ends_with(".db") {
        // DB-only restore
        std::fs::copy(&source_path, &db_path)
            .map_err(|e| format!("Failed to copy DB: {}", e))?;
        Ok("Database restored successfully! Restart the app for changes to take effect.".to_string())
    } else {
        Err("Unsupported backup file type. Only .db and .zip supported.".to_string())
    }
}

/// v0.22.0: Import products from a catalog.json file (URL or local path).
///
/// Ali bhai's requirement after data loss: recover product data from published
/// catalog.json (frontend) if local DB is lost/corrupt.
///
/// Behavior:
/// - Fetches catalog.json from given URL (default: live catalog)
/// - Parses products array
/// - For each product: INSERT into products table (OR IGNORE to skip existing SKUs)
/// - Returns summary (total imported, skipped, failed)
///
/// This is a RECOVERY feature. It will restore: name, SKU, price, retail_price,
/// category, color, fabric, season, description, images.
/// It will NOT restore: cost_price, supplier, sales history, customers, agents,
/// ledger entries — those are private and not in catalog.json.
#[tauri::command]
pub async fn import_from_catalog_json(
    state: State<'_, DbState>,
    catalog_url: Option<String>,
) -> Result<serde_json::Value, String> {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct CatalogProduct {
        id: i64,
        name: String,
        sku: Option<String>,
        sale_price: f64,
        retail_price: Option<f64>,
        category: Option<String>,
        color: Option<String>,
        fabric: Option<String>,
        season: Option<String>,
        description: Option<String>,
        images: Vec<String>,
        availability: String,
    }

    #[derive(Debug, Deserialize)]
    struct CatalogJsonImport {
        brand: Option<String>,
        whatsapp_number: Option<String>,
        products: Vec<CatalogProduct>,
    }

    // Default URL: live catalog
    let url = catalog_url.unwrap_or_else(|| {
        "https://airdropia.github.io/a-collection-catalog/data/catalog.json".to_string()
    });

    // Derive the catalog's image base URL from the catalog.json URL.
    // catalog.json lives at <base>/data/catalog.json — images live at <base>/data/images/<filename>.
    // We swap the trailing "catalog.json" with "images/".
    let image_base_url = url
        .rsplitn(2, "catalog.json")
        .last()
        .unwrap_or("")
        .to_string() + "images/";

    // Fetch JSON
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("a-collection-head-office/0.22.5")
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    let response = client.get(&url).send().await
        .map_err(|e| format!("Failed to fetch catalog.json: {}", e))?;
    let body = response.text().await
        .map_err(|e| format!("Failed to read response: {}", e))?;
    let catalog: CatalogJsonImport = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse catalog.json: {}", e))?;

    // ====================================================================
    // Phase 1a: Acquire DB lock briefly to filter out SKUs that already exist.
    // Lock is dropped BEFORE any HTTP `.await` for image downloads (Rule #12:
    // NEVER hold DB Mutex across `.await`).
    // ====================================================================
    let existing_skus: std::collections::HashSet<String> = {
        let conn = state.0.lock().await;
        let mut set = std::collections::HashSet::new();
        for product in &catalog.products {
            if let Some(sku) = product.sku.as_ref() {
                if !sku.is_empty() {
                    let exists: Option<i64> = (&*conn).query_row(
                        "SELECT id FROM products WHERE sku = ?1",
                        rusqlite::params![sku],
                        |r| r.get(0),
                    ).ok();
                    if exists.is_some() {
                        set.insert(sku.clone());
                    }
                }
            }
        }
        set
    }; // ← DB lock dropped here

    // Build the list of products to import (skip duplicates).
    // Pre-compute the per-product import data so Phase 1b/2 don't re-read
    // catalog.products with filter logic.
    struct ProductToImport<'a> {
        product: &'a CatalogProduct,
        images_json: String,
        qty_in_ho: i64,
        profit_status: Option<String>,
    }

    let mut to_import: Vec<ProductToImport> = Vec::new();
    let mut skipped = 0;
    for product in &catalog.products {
        let sku = product.sku.as_deref().unwrap_or("");
        if !sku.is_empty() && existing_skus.contains(sku) {
            skipped += 1;
            continue;
        }

        let profit_status = if product.availability == "sold_out" {
            Some("sold_out".to_string())
        } else if product.availability == "low_stock" {
            Some("in_head_office".to_string())
        } else {
            None
        };

        let qty_in_ho: i64 = if product.availability == "sold_out" { 0 } else { 3 };

        let images_json = serde_json::to_string(&product.images)
            .unwrap_or_else(|_| "[]".to_string());

        to_import.push(ProductToImport {
            product,
            images_json,
            qty_in_ho,
            profit_status,
        });
    }

    // ====================================================================
    // Phase 1b: Download all product images to AppData/images/.
    // No DB lock held — HTTP I/O only. Skips files that already exist on disk
    // (so re-running import after a partial failure doesn't re-download).
    // Failures are logged but don't abort the import — product text data
    // still lands in the DB. Ali bhai can manually add missing images later
    // via the Edit Product form.
    // ====================================================================
    let images_dir = crate::utils::get_images_dir();
    let mut images_downloaded = 0u32;
    let mut images_skipped = 0u32;
    let mut image_errors: Vec<String> = Vec::new();

    for item in &to_import {
        for filename in &item.product.images {
            if filename.is_empty() {
                continue;
            }
            let dest_path = images_dir.join(filename);
            if dest_path.exists() {
                images_skipped += 1;
                continue;
            }

            let img_url = format!("{}{}", image_base_url, filename);
            match client.get(&img_url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        match resp.bytes().await {
                            Ok(bytes) => {
                                if let Err(e) = std::fs::write(&dest_path, &bytes) {
                                    image_errors.push(
                                        format!("{}: write failed: {}", filename, e)
                                    );
                                } else {
                                    images_downloaded += 1;
                                }
                            }
                            Err(e) => image_errors.push(
                                format!("{}: read body failed: {}", filename, e)
                            ),
                        }
                    } else {
                        image_errors.push(
                            format!("{}: HTTP {}", filename, resp.status())
                        );
                    }
                }
                Err(e) => image_errors.push(
                    format!("{}: fetch failed: {}", filename, e)
                ),
            }
        }
    }

    // ====================================================================
    // Phase 2: Acquire DB lock and INSERT all products.
    // No `.await` inside this block — pure sync rusqlite calls.
    // ====================================================================
    let now = chrono::Utc::now().to_rfc3339();
    let mut imported = 0u32;
    let mut failed = 0u32;
    let mut errors: Vec<String> = Vec::new();

    {
        let conn = state.0.lock().await;

        for item in &to_import {
            let product = item.product;
            // 22 columns, 22 placeholders (?1..?22), 22 params — all in same order.
            // No `design` column here because catalog.json does not carry it.
            // created_at + updated_at use distinct slots (?21, ?22) but share
            // the same `&now` value.
            match (&*conn).execute(
                "INSERT INTO products (sku, name, category, color, season, description, images,
                    cost_price, sale_price, purchase_price, stock_quantity, status,
                    qty_in_head_office, qty_with_agents, qty_sold, qty_reserved, profit_status,
                    retail_price, brand, fabric, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                rusqlite::params![
                    product.sku.as_deref().unwrap_or(""),                  // ?1
                    &product.name,                                          // ?2
                    product.category.as_deref().unwrap_or(""),              // ?3
                    product.color.as_deref().unwrap_or(""),                 // ?4
                    product.season.as_deref().unwrap_or(""),                // ?5
                    product.description.as_deref().unwrap_or(&product.name),// ?6
                    &item.images_json,                                      // ?7
                    0.0,                                                    // ?8  cost_price (private)
                    product.sale_price,                                     // ?9
                    product.sale_price,                                     // ?10 purchase_price fallback
                    item.qty_in_ho,                                         // ?11 stock_quantity
                    "active",                                               // ?12 status
                    item.qty_in_ho,                                         // ?13 qty_in_head_office
                    0,                                                      // ?14 qty_with_agents
                    0,                                                      // ?15 qty_sold
                    0,                                                      // ?16 qty_reserved
                    &item.profit_status,                                    // ?17 profit_status
                    product.retail_price,                                   // ?18 retail_price
                    catalog.brand.as_deref().unwrap_or(""),                 // ?19 brand
                    product.fabric.as_deref().unwrap_or(""),                // ?20 fabric
                    &now,                                                   // ?21 created_at
                    &now,                                                   // ?22 updated_at (same value, distinct slot)
                ],
            ) {
                Ok(_) => imported += 1,
                Err(e) => {
                    failed += 1;
                    errors.push(format!("{}: {}", product.name, e));
                    if errors.len() > 5 { break; }  // Limit error list
                }
            }
        }
    } // ← DB lock dropped here

    Ok(serde_json::json!({
        "total_in_catalog": catalog.products.len(),
        "imported": imported,
        "skipped": skipped,
        "failed": failed,
        "errors": errors,
        "brand": catalog.brand.unwrap_or_default(),
        "whatsapp_number": catalog.whatsapp_number.unwrap_or_default(),
        "images_downloaded": images_downloaded,
        "images_skipped_existing": images_skipped,
        "image_errors": image_errors,
    }))
}

/// Re-run database migrations on demand. This is primarily used by the
/// Locations page's "Sync from Profile" button to trigger the
/// `sync_sales_areas_to_locations` migration step without requiring an
/// app restart. Returns Ok(()) on success.
#[tauri::command]
pub async fn init_database(state: State<'_, DbState>) -> Result<(), String> {
    let mut conn = state.0.lock().await;
    // Re-run migrations by calling run_migrations directly. This is safe
    // because all migration steps are idempotent (CREATE TABLE IF NOT EXISTS,
    // INSERT OR IGNORE, add_col_if_missing).
    crate::database::run_migrations_public(&mut conn).map_err(|e| e.to_string())
}
