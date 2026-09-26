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
// // Share logs + customer segments + stale products
// ============================================================

// ============================================================
// v0.11.1 — Share Center (share_logs + customer segments)
// ============================================================

/// Log a share action. Called whenever the user shares a product to a
/// social platform. Creates an audit trail entry in share_logs.
#[tauri::command]
pub async fn log_share(
    state: State<'_, DbState>,
    product_id: Option<i64>,
    platform: String,
    share_angle: Option<String>,
    caption_text: Option<String>,
    notes: Option<String>,
) -> Result<i64, String> {
    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();
    // 'shared_by' is hardcoded to 'Head Office' for now. Future: track
    // which user/device shared (when multi-user support is added).
    conn.execute(
        "INSERT INTO share_logs (product_id, platform, share_angle, caption_text, shared_by, shared_at, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            product_id,
            &platform,
            share_angle.as_deref().unwrap_or(""),
            caption_text.as_deref().unwrap_or(""),
            "Head Office",
            &now,
            notes.as_deref().unwrap_or(""),
        ],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

/// Get recent share logs. Returns up to `limit` most recent entries.
/// Optionally filter by product_id (if provided).
#[tauri::command]
pub async fn get_share_logs(
    state: State<'_, DbState>,
    product_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.0.lock().await;
    let limit = limit.unwrap_or(50);

    if let Some(pid) = product_id {
        let mut s = conn.prepare(
            "SELECT sl.id, sl.product_id, sl.platform, sl.share_angle, sl.caption_text, sl.shared_by, sl.shared_at, sl.notes,
                    COALESCE(p.name, '(deleted)') AS product_name
             FROM share_logs sl
             LEFT JOIN products p ON sl.product_id = p.id
             WHERE sl.product_id = ?1
             ORDER BY sl.shared_at DESC, sl.id DESC
             LIMIT ?2"
        ).map_err(|e| e.to_string())?;
        let rows = s.query_map(rusqlite::params![pid, limit], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "product_id": row.get::<_, Option<i64>>(1)?,
                "platform": row.get::<_, String>(2)?,
                "share_angle": row.get::<_, String>(3)?,
                "caption_text": row.get::<_, String>(4)?,
                "shared_by": row.get::<_, String>(5)?,
                "shared_at": row.get::<_, String>(6)?,
                "notes": row.get::<_, String>(7)?,
                "product_name": row.get::<_, String>(8)?,
            }))
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for r in rows { result.push(r.map_err(|e| e.to_string())?); }
        return Ok(result);
    } else {
        let mut s = conn.prepare(
            "SELECT sl.id, sl.product_id, sl.platform, sl.share_angle, sl.caption_text, sl.shared_by, sl.shared_at, sl.notes,
                    COALESCE(p.name, '(deleted)') AS product_name
             FROM share_logs sl
             LEFT JOIN products p ON sl.product_id = p.id
             ORDER BY sl.shared_at DESC, sl.id DESC
             LIMIT ?1"
        ).map_err(|e| e.to_string())?;
        let rows = s.query_map(rusqlite::params![limit], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "product_id": row.get::<_, Option<i64>>(1)?,
                "platform": row.get::<_, String>(2)?,
                "share_angle": row.get::<_, String>(3)?,
                "caption_text": row.get::<_, String>(4)?,
                "shared_by": row.get::<_, String>(5)?,
                "shared_at": row.get::<_, String>(6)?,
                "notes": row.get::<_, String>(7)?,
                "product_name": row.get::<_, String>(8)?,
            }))
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for r in rows { result.push(r.map_err(|e| e.to_string())?); }
        return Ok(result);
    }
}

/// Get customers filtered by segment. Used by the Share Center's bulk
/// WhatsApp broadcast feature.
#[tauri::command]
pub async fn get_customers_by_segment(
    state: State<'_, DbState>,
    segment: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.0.lock().await;

    if let Some(seg) = segment {
        let mut s = conn.prepare(
            "SELECT id, name, phone, location, notes, segment, is_active
             FROM customers
             WHERE segment = ?1 AND is_active = 1
             ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let rows = s.query_map(rusqlite::params![seg], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "phone": row.get::<_, Option<String>>(2)?,
                "location": row.get::<_, Option<String>>(3)?,
                "notes": row.get::<_, Option<String>>(4)?,
                "segment": row.get::<_, String>(5)?,
                "is_active": row.get::<_, i64>(6)? != 0,
            }))
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for r in rows { result.push(r.map_err(|e| e.to_string())?); }
        Ok(result)
    } else {
        let mut s = conn.prepare(
            "SELECT id, name, phone, location, notes, segment, is_active
             FROM customers
             WHERE is_active = 1
             ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let rows = s.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "phone": row.get::<_, Option<String>>(2)?,
                "location": row.get::<_, Option<String>>(3)?,
                "notes": row.get::<_, Option<String>>(4)?,
                "segment": row.get::<_, String>(5)?,
                "is_active": row.get::<_, i64>(6)? != 0,
            }))
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for r in rows { result.push(r.map_err(|e| e.to_string())?); }
        Ok(result)
    }
}

/// Update a customer's segment. Used by the Customers page to assign
/// segments (women, girls, vip, agent, etc.) for bulk broadcasting.
#[tauri::command]
pub async fn update_customer_segment(
    state: State<'_, DbState>,
    customer_id: i64,
    segment: String,
) -> Result<(), String> {
    let conn = state.0.lock().await;
    conn.execute(
        "UPDATE customers SET segment = ?1 WHERE id = ?2",
        rusqlite::params![&segment, customer_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Get all distinct customer segments (for populating the segment filter
/// dropdown in the Share Center).
#[tauri::command]
pub async fn get_customer_segments(state: State<'_, DbState>) -> Result<Vec<String>, String> {
    let conn = state.0.lock().await;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT segment FROM customers WHERE segment IS NOT NULL AND segment != '' ORDER BY segment"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for r in rows { result.push(r.map_err(|e| e.to_string())?); }
    Ok(result)
}

/// Get products that have NOT been shared in the last X days (or never
/// shared). Used by the Share Center's "Stale Stock" detector.
#[tauri::command]
pub async fn get_stale_products(
    state: State<'_, DbState>,
    days: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.0.lock().await;
    let days = days.unwrap_or(7);
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    // Products where: status = active AND (no share_log exists OR most
    // recent share_log is older than cutoff).
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.sku, p.sale_price, p.stock_quantity,
                COALESCE(p.images, '[]') AS images,
                MAX(sl.shared_at) AS last_shared_at
         FROM products p
         LEFT JOIN share_logs sl ON sl.product_id = p.id
         WHERE p.status = 'active' AND p.stock_quantity > 0
         GROUP BY p.id, p.name, p.sku, p.sale_price, p.stock_quantity, p.images
         HAVING MAX(sl.shared_at) IS NULL OR MAX(sl.shared_at) < ?1
         ORDER BY (MAX(sl.shared_at) IS NULL) DESC, p.name ASC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params![&cutoff], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, i64>(0)?,
            "name": row.get::<_, String>(1)?,
            "sku": row.get::<_, String>(2)?,
            "sale_price": row.get::<_, f64>(3)?,
            "stock_quantity": row.get::<_, i64>(4)?,
            "images": row.get::<_, String>(5)?,
            "last_shared_at": row.get::<_, Option<String>>(6)?,
        }))
    }).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for r in rows { result.push(r.map_err(|e| e.to_string())?); }
    Ok(result)
}
