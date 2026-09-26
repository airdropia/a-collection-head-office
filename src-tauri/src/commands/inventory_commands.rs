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
// // Inventory: summary, low/dead stock, best sellers, adjust
// ============================================================

// ==================== INVENTORY ====================

#[tauri::command]
pub async fn get_inventory_summary(state: State<'_, DbState>) -> Result<InventorySummary, String> {
    let conn = state.0.lock().await;
    inventory::get_inventory_summary(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_low_stock(state: State<'_, DbState>, threshold: i64) -> Result<Vec<LowStockItem>, String> {
    let conn = state.0.lock().await;
    inventory::get_low_stock_items(&conn, threshold).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_dead_stock(state: State<'_, DbState>, days_limit: i64) -> Result<Vec<DeadStockItem>, String> {
    let conn = state.0.lock().await;
    inventory::get_dead_stock_items(&conn, days_limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_best_sellers(state: State<'_, DbState>, limit: i64) -> Result<Vec<BestSellerItem>, String> {
    let conn = state.0.lock().await;
    inventory::get_best_sellers(&conn, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn adjust_stock(state: State<'_, DbState>, product_id: i64, adjustment: i64) -> Result<(), String> {
    let conn = state.0.lock().await;
    inventory::adjust_stock(&conn, product_id, adjustment).map_err(|e| e.to_string())
}
