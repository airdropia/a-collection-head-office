//! Auto-extracted from commands/mod.rs during v0.27.0 refactor.
//! See worklog.md Task ID: refactor-1 for context.
//!
//! Behavior unchanged — only file structure modified.

use crate::catalog::{self, Product};
use crate::inventory::{self, InventorySummary, LowStockItem, DeadStockItem, BestSellerItem};
use crate::customers::{self, Customer, OrderItemInput, OrderHistory};
use crate::reports::{self, SalesReport, InventoryReport, CustomerSummaryReport};
use crate::purchase_trips::{self, PurchaseTripSummary};
use crate::utils;
use crate::commands::{DbState, set_setting_val, get_setting_val};
use tauri::async_runtime::Mutex;
use std::path::Path;
use rusqlite::{Connection, params};
use tauri::State;

// ============================================================
// // Catalog Publishing: preview + publish + history
// ============================================================

// ============================================================
// v0.15.0: PUBLIC CATALOG PUBLISHING
// ============================================================

/// Preview what will be published to the public catalog.
/// Returns stats (product count, image count, total size, catalog URL).
/// Called by the UI to show a confirmation modal before publishing.
///
/// v0.15.2: Critical — must acquire DB lock, read settings + products, then
/// RELEASE the lock before doing any async work. rusqlite::Connection is not
/// Send, so holding the Mutex across .await causes "future cannot be sent
/// between threads safely" compile error. Standard pattern: lock → read →
/// drop → then await.
#[tauri::command]
pub async fn preview_catalog_publish(
    state: State<'_, DbState>,
) -> Result<crate::catalog_publish::CatalogPreview, String> {
    // Scope the lock — read all needed data, then release before any .await
    let (brand, whatsapp, repo, preview) = {
        let conn = state.0.lock().await;

        let get_setting = |key: &str| -> String {
            conn.query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            ).unwrap_or_default()
        };

        let brand = {
            let v = get_setting("catalog_brand");
            if v.is_empty() { "A Collection Narowal".to_string() } else { v }
        };
        let whatsapp = {
            let v = get_setting("catalog_whatsapp");
            if v.is_empty() { "923420830995".to_string() } else { v }
        };
        let repo = {
            let v = get_setting("catalog_repo");
            if v.is_empty() { "airdropia/a-collection-catalog".to_string() } else { v }
        };

        // build_preview does file I/O (reading image sizes) but no async work,
        // so it's safe to call while holding the lock.
        let preview = crate::catalog_publish::build_preview(&conn, &brand, &whatsapp, &repo)?;
        (brand, whatsapp, repo, preview)
    };
    // Lock released here

    Ok(preview)
}

/// Publish catalog to GitHub via Contents API.
/// v0.15.2: Same pattern as preview_catalog_publish — lock, read, release,
/// then do async GitHub API calls. Connection is not Send so can't be held
/// across .await.
/// v0.16.0: Now logs every publish attempt to catalog_publish_history table.
#[tauri::command]
pub async fn publish_catalog_to_github(
    state: State<'_, DbState>,
) -> Result<crate::catalog_publish::PublishResult, String> {
    let start_time = std::time::Instant::now();

    // Phase 1: Acquire lock, read all settings + build catalog data, release lock
    let (catalog, image_mapping, repo, github_token, warnings_count, errors_count) = {
        let conn = state.0.lock().await;

        let get_setting = |key: &str| -> String {
            conn.query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            ).unwrap_or_default()
        };

        let brand = {
            let v = get_setting("catalog_brand");
            if v.is_empty() { "A Collection Narowal".to_string() } else { v }
        };
        let whatsapp = {
            let v = get_setting("catalog_whatsapp");
            if v.is_empty() { "923420830995".to_string() } else { v }
        };
        let repo = {
            let v = get_setting("catalog_repo");
            if v.is_empty() { "airdropia/a-collection-catalog".to_string() } else { v }
        };
        let github_token = get_setting("catalog_github_token");
        if github_token.is_empty() {
            return Err("GitHub token not configured. Go to Settings → Catalog and paste your GitHub Personal Access Token.".to_string());
        }

        // Build catalog data (sync — safe to call while holding lock)
        let mut catalog = crate::catalog_publish::build_catalog_json(&conn, &brand, &whatsapp)
            .map_err(|e| format!("Failed to build catalog: {}", e))?;
        let image_mapping = crate::catalog_publish::generate_webp_images(&conn)
            .map_err(|e| format!("Failed to generate images: {}", e))?;

        // v0.16.2: CRITICAL FIX — catalog.json stores original image filenames
        // (e.g. "123456_thumbnail.jpg") but uploaded files have catalog names
        // (e.g. "123456_catalog.jpg"). Without this rewrite, the frontend tries
        // to load data/images/123456_thumbnail.jpg but the file on GitHub is
        // data/images/123456_catalog.jpg → broken images!
        for product in &mut catalog.products {
            let mut catalog_images: Vec<String> = Vec::new();
            for orig_img in &product.images {
                if let Some(catalog_img) = image_mapping.get(orig_img) {
                    catalog_images.push(catalog_img.clone());
                } else {
                    catalog_images.push(orig_img.clone());
                }
            }
            product.images = catalog_images;
        }

        // v0.16.0: Get validation counts for history logging
        let (warnings_count, errors_count) = match crate::catalog_publish::build_preview(&conn, &brand, &whatsapp, &repo) {
            Ok(p) => (p.warnings.len() as i64, p.errors.len() as i64),
            Err(_) => (0, 0),
        };

        (catalog, image_mapping, repo, github_token, warnings_count, errors_count)
    };
    // Lock released here — now safe to do async GitHub API work

    // Phase 2: Upload to GitHub (no DB access, pure async HTTP)
    let result = crate::catalog_publish::upload_to_github(
        &catalog, &image_mapping, &repo, &github_token,
    ).await;

    let duration_ms = start_time.elapsed().as_millis() as i64;

    // Phase 3: Log to history (acquire lock briefly)
    {
        let conn = state.0.lock().await;
        let (success, error_msg) = match &result {
            Ok(r) => (r.success, if r.errors.is_empty() { None } else { Some(r.errors.join("; ")) }),
            Err(e) => (false, Some(e.clone())),
        };
        let products_count = catalog.products.len() as i64;
        let images_uploaded = result.as_ref().map(|r| r.images_uploaded as i64).unwrap_or(0);
        let images_deleted = result.as_ref().map(|r| r.images_deleted as i64).unwrap_or(0);
        let catalog_version = catalog.version.clone();

        let _ = crate::catalog_publish::log_publish_history(
            &conn,
            duration_ms,
            products_count,
            images_uploaded,
            images_deleted,
            success,
            Some(&catalog_version),
            error_msg.as_deref(),
            warnings_count,
            errors_count,
        );
    }

    result
}

/// v0.16.0: Get the last N publish history entries (most recent first).
/// Used by the UI to show publish status + history in Settings.
#[tauri::command]
pub async fn get_catalog_publish_history(
    state: State<'_, DbState>,
    limit: Option<i64>,
) -> Result<Vec<crate::catalog_publish::PublishHistoryEntry>, String> {
    let conn = state.0.lock().await;
    let limit = limit.unwrap_or(10);
    crate::catalog_publish::get_publish_history(&conn, limit).map_err(|e| e.to_string())
}
