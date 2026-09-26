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
// // Purchase Trips: CRUD + items
// ============================================================

// ============================================================
// v0.11.2 — Purchase Trips (landed cost tracking)
// ============================================================

#[tauri::command]
pub async fn get_purchase_trips(state: State<'_, DbState>) -> Result<Vec<PurchaseTripSummary>, String> {
    let conn = state.0.lock().await;
    purchase_trips::get_all_purchase_trips(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_purchase_trip(state: State<'_, DbState>, id: i64) -> Result<serde_json::Value, String> {
    let conn = state.0.lock().await;
    let (trip, items) = purchase_trips::get_purchase_trip(&conn, id).map_err(|e| e.to_string())?;
    // Enrich items with product names
    let mut enriched_items: Vec<serde_json::Value> = Vec::new();
    for item in items {
        let product_name: Option<String> = if let Some(pid) = item.product_id {
            conn.query_row(
                "SELECT name FROM products WHERE id = ?1",
                rusqlite::params![pid],
                |r| r.get(0),
            ).ok()
        } else {
            None
        };
        enriched_items.push(serde_json::json!({
            "id": item.id,
            "trip_id": item.trip_id,
            "product_id": item.product_id,
            "product_name": product_name.unwrap_or_else(|| "(deleted)".to_string()),
            "qty_purchased": item.qty_purchased,
            "unit_purchase_cost": item.unit_purchase_cost,
            "total_purchase_cost": item.total_purchase_cost,
            "expense_allocation_amount": item.expense_allocation_amount,
            "landed_unit_cost": item.landed_unit_cost,
        }));
    }
    Ok(serde_json::json!({
        "trip": trip,
        "items": enriched_items,
    }))
}

#[tauri::command]
pub async fn create_purchase_trip(
    state: State<'_, DbState>,
    trip_date: String,
    source_city: Option<String>,
    supplier_notes: Option<String>,
    travel_cost: Option<f64>,
    transport_cost: Option<f64>,
    food_cost: Option<f64>,
    loading_cost: Option<f64>,
    misc_cost: Option<f64>,
) -> Result<i64, String> {
    let conn = state.0.lock().await;
    purchase_trips::create_purchase_trip(
        &conn,
        &trip_date,
        source_city.as_deref().unwrap_or("Faisalabad"),
        supplier_notes.as_deref(),
        travel_cost.unwrap_or(0.0),
        transport_cost.unwrap_or(0.0),
        food_cost.unwrap_or(0.0),
        loading_cost.unwrap_or(0.0),
        misc_cost.unwrap_or(0.0),
    ).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_purchase_trip(
    state: State<'_, DbState>,
    id: i64,
    trip_date: String,
    source_city: String,
    supplier_notes: Option<String>,
    travel_cost: f64,
    transport_cost: f64,
    food_cost: f64,
    loading_cost: f64,
    misc_cost: f64,
) -> Result<(), String> {
    let conn = state.0.lock().await;
    purchase_trips::update_purchase_trip(
        &conn, id, &trip_date, &source_city, supplier_notes.as_deref(),
        travel_cost, transport_cost, food_cost, loading_cost, misc_cost,
    ).map_err(|e| e.to_string())?;
    // Recalculate allocations since expenses may have changed
    purchase_trips::recalculate_trip_allocations(&conn, id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_purchase_trip(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().await;
    purchase_trips::delete_purchase_trip(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_trip_item(
    state: State<'_, DbState>,
    trip_id: i64,
    product_id: i64,
    qty_purchased: i64,
    unit_purchase_cost: f64,
) -> Result<i64, String> {
    if qty_purchased <= 0 {
        return Err("Quantity must be positive.".to_string());
    }
    let conn = state.0.lock().await;
    let item_id = purchase_trips::add_trip_item(
        &conn, trip_id, product_id, qty_purchased, unit_purchase_cost,
    ).map_err(|e| e.to_string())?;
    // Recalculate allocations for the whole trip (new item changes proportions)
    purchase_trips::recalculate_trip_allocations(&conn, trip_id).map_err(|e| e.to_string())?;
    Ok(item_id)
}

#[tauri::command]
pub async fn remove_trip_item(state: State<'_, DbState>, item_id: i64) -> Result<(), String> {
    let conn = state.0.lock().await;
    // Get trip_id before deleting so we can recalculate after
    let trip_id: Option<i64> = conn.query_row(
        "SELECT trip_id FROM purchase_trip_items WHERE id = ?1",
        rusqlite::params![item_id],
        |r| r.get(0),
    ).ok();
    purchase_trips::remove_trip_item(&conn, item_id).map_err(|e| e.to_string())?;
    if let Some(tid) = trip_id {
        purchase_trips::recalculate_trip_allocations(&conn, tid).map_err(|e| e.to_string())?;
    }
    Ok(())
}
