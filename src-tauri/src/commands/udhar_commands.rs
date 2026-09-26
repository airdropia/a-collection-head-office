//! Auto-extracted from commands/mod.rs during v0.27.0 refactor.
//! See worklog.md Task ID: refactor-1 for context.
//!
//! Behavior unchanged — only file structure modified.

use crate::catalog::{self, Product};
use crate::inventory::{self, InventorySummary, LowStockItem, DeadStockItem, BestSellerItem};
use crate::customers::{self, Customer};
use crate::reports::{self, SalesReport, InventoryReport, CustomerSummaryReport};
use crate::utils;
use crate::commands::{DbState, set_setting_val, get_setting_val};
use tauri::async_runtime::Mutex;
use std::path::Path;
use rusqlite::{Connection, params};
use tauri::State;

// ============================================================
// // Customer Udhar/Credit (खाता): record_payment + balance_history
// ============================================================

// ============================================================
// v0.26.0: CUSTOMER UDHAR / CREDIT (खाता) TRACKING
// ============================================================

/// Record a payment from a customer against their outstanding balance.
/// Reduces customer.outstanding_balance by the payment amount and inserts
/// a row in customer_payments for audit/history.
/// Optionally link the payment to a specific sale_id.
///
/// v0.35.0: business logic extracted to customers::record_payment_impl —
/// shared verbatim with the acollectionho write-CLI (Phase B).
#[tauri::command]
pub async fn record_customer_payment(
    state: State<'_, DbState>,
    customer_id: i64,
    amount: f64,
    notes: Option<String>,
    sale_id: Option<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().await;
    customers::record_payment_impl(&conn, customer_id, amount, notes.as_deref(), sale_id)
}

/// Get a customer's full balance history (sales + payments) in chronological order.
/// Each entry includes: type (sale/payment), date, description, amount, balance_after.
/// Used by the customer detail modal to show the khata timeline.
#[tauri::command]
pub async fn get_customer_balance_history(
    state: State<'_, DbState>,
    customer_id: i64,
) -> Result<Vec<customers::BalanceHistoryEntry>, String> {
    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();

    // Fetch sales with balance for this customer
    let mut stmt = conn.prepare(
        "SELECT s.id, s.sale_date, p.name, s.qty, s.total_sale_amount, s.balance
         FROM sales s
         LEFT JOIN products p ON s.product_id = p.id
         WHERE s.customer_id = ?1
         ORDER BY s.sale_date ASC"
    ).map_err(|e| e.to_string())?;

    let sales: Vec<(i64, String, String, i64, f64, f64)> = stmt.query_map(
        [customer_id],
        |row| Ok((
            row.get(0)?,
            row.get(1)?,
            row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
        )),
    ).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    // Fetch payments for this customer
    // v0.29.0: Now includes entry_type so we can distinguish:
    //   payment, opening_debit, adjustment in the history timeline.
    let mut stmt2 = conn.prepare(
        "SELECT id, payment_date, amount, notes, entry_type
         FROM customer_payments
         WHERE customer_id = ?1
         ORDER BY payment_date ASC"
    ).map_err(|e| e.to_string())?;

    let payments: Vec<(i64, String, f64, String, String)> = stmt2.query_map(
        [customer_id],
        |row| Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "payment".to_string()),
        )),
    ).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    // Merge + sort by date, compute running balance
    let mut entries: Vec<customers::BalanceHistoryEntry> = Vec::new();
    let mut running_balance: f64 = 0.0;

    // Build a combined list of (date, type, description, amount_change)
    let mut combined: Vec<(String, String, String, f64, i64)> = Vec::new();
    for (id, date, pname, qty, total, balance) in &sales {
        let desc = format!("{} x{} (Rs. {:.0})", pname, qty, total);
        combined.push((date.clone(), "sale".to_string(), desc, *balance, *id));
    }
    for (id, date, amount, notes, etype) in &payments {
        let (desc, amount_change) = match etype.as_str() {
            "opening_debit" => {
                let d = if notes.is_empty() {
                    format!("Opening Balance: Rs. {:.0}", amount)
                } else {
                    format!("Opening Balance: {} (Rs. {:.0})", notes, amount)
                };
                (d, *amount)
            }
            "adjustment" => {
                let d = if notes.is_empty() {
                    format!("Adjustment: Rs. {:.0}", amount)
                } else {
                    format!("Adjustment: {} (Rs. {:.0})", notes, amount)
                };
                (d, *amount) // signed; can be negative
            }
            _ => {
                // payment (existing behavior)
                let d = if notes.is_empty() {
                    format!("Payment Rs. {:.0}", amount)
                } else {
                    format!("Payment: {}", notes)
                };
                (d, -*amount)
            }
        };
        combined.push((date.clone(), etype.clone(), desc, amount_change, *id));
    }
    combined.sort_by(|a, b| a.0.cmp(&b.0));

    for (date, etype, desc, amount_change, id) in combined {
        running_balance += amount_change;
        if running_balance < 0.0 { running_balance = 0.0; } // safety
        entries.push(customers::BalanceHistoryEntry {
            id,
            entry_type: etype,
            date,
            description: desc,
            amount: amount_change,
            balance_after: running_balance,
        });
    }

    let _ = now; // suppress unused warning
    Ok(entries)
}

// ============================================================
// v0.29.0: MANUAL LEDGER ENTRIES (opening balance + adjustments)
// ============================================================
//
// These commands complement record_customer_payment (which only handles
// normal payments against outstanding balance). They allow bhai to:
//
//   1. Add an opening_debit entry when onboarding a customer whose old
//      udhar wasn't tracked (e.g., "Sami Amjad ka purana 9400 rqaya").
//   2. Add an adjustment entry for arbitrary corrections — discount,
//      write-off, error fix, or "maal value" without a linked sale.
//
// All entries go into the same customer_payments table (now with an
// entry_type column). The customer's outstanding_balance is updated
// atomically within a transaction.
//
// Edit + delete operations recompute the customer's outstanding_balance
// from scratch (SUM of all entries) to ensure consistency.

/// Add a manual ledger entry for a customer.
///
/// - entry_type = "opening_debit": amount must be > 0, increases balance
/// - entry_type = "adjustment": amount can be + (customer owes more) or
///   - (customer owes less, e.g., discount)
///
/// `entry_date` is optional — defaults to now (UTC ISO 8601).
///
/// v0.35.0: business logic extracted to customers::add_manual_entry_impl —
/// shared verbatim with the acollectionho write-CLI (Phase B).
#[tauri::command]
pub async fn add_customer_ledger_entry(
    state: State<'_, DbState>,
    customer_id: i64,
    entry_type: String,
    amount: f64,
    entry_date: Option<String>,
    notes: Option<String>,
) -> Result<i64, String> {
    // Validate entry_type
    if entry_type != "opening_debit" && entry_type != "adjustment" {
        return Err(format!(
            "Invalid entry_type '{}'. Must be 'opening_debit' or 'adjustment'.",
            entry_type
        ));
    }
    // opening_debit must be positive
    if entry_type == "opening_debit" && amount <= 0.0 {
        return Err("Opening debit amount must be positive.".to_string());
    }
    // adjustment can be zero — no-op, but allowed
    if entry_type == "adjustment" && amount == 0.0 {
        return Err("Adjustment amount cannot be zero.".to_string());
    }

    let conn = state.0.lock().await;
    customers::add_manual_entry_impl(
        &conn,
        customer_id,
        &entry_type,
        amount,
        notes.as_deref(),
        entry_date.as_deref(),
    )
}

/// Update an existing manual ledger entry (opening_debit or adjustment).
/// Normal 'payment' entries are also editable but with caution — they may
/// be linked to a sale. Sale entries are NOT editable here.
///
/// Recomputes the customer's outstanding_balance from scratch after update
/// to ensure consistency.
#[tauri::command]
pub async fn update_customer_ledger_entry(
    state: State<'_, DbState>,
    entry_id: i64,
    amount: f64,
    notes: Option<String>,
    entry_date: Option<String>,
) -> Result<(), String> {
    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute("BEGIN IMMEDIATE", []).map_err(|e| e.to_string())?;

    // Fetch existing entry
    let (customer_id, entry_type): (i64, String) = conn.query_row(
        "SELECT customer_id, entry_type FROM customer_payments WHERE id = ?1",
        rusqlite::params![entry_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        format!("Ledger entry not found: {}", e)
    })?;

    // Validate based on entry_type
    if entry_type == "opening_debit" && amount <= 0.0 {
        let _ = conn.execute("ROLLBACK", []);
        return Err("Opening debit amount must be positive.".to_string());
    }
    if entry_type == "adjustment" && amount == 0.0 {
        let _ = conn.execute("ROLLBACK", []);
        return Err("Adjustment amount cannot be zero.".to_string());
    }

    // Update the entry
    let date_clause = if let Some(d) = &entry_date {
        "payment_date = ?4, "
    } else {
        ""
    };
    let sql = format!(
        "UPDATE customer_payments SET amount = ?1, notes = ?2, {}updated_at = ?3 WHERE id = ?5",
        date_clause
    );
    let res = if entry_date.is_some() {
        conn.execute(
            &sql,
            rusqlite::params![amount, notes.as_deref().unwrap_or(""), &now, entry_date.as_ref().unwrap(), entry_id],
        )
    } else {
        conn.execute(
            &sql,
            rusqlite::params![amount, notes.as_deref().unwrap_or(""), &now, entry_id],
        )
    };
    if let Err(e) = res {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to update entry: {}", e));
    }

    // Recompute customer's outstanding_balance from all entries
    recompute_customer_balance(&conn, customer_id)?;

    conn.execute("COMMIT", []).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        e.to_string()
    })?;

    Ok(())
}

/// Delete a manual ledger entry. Recomputes customer's outstanding_balance
/// from scratch after deletion. Sale entries cannot be deleted here —
/// delete the sale instead.
#[tauri::command]
pub async fn delete_customer_ledger_entry(
    state: State<'_, DbState>,
    entry_id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().await;

    conn.execute("BEGIN IMMEDIATE", []).map_err(|e| e.to_string())?;

    // Fetch entry to delete + validate it's not a sale
    let (customer_id, entry_type): (i64, String) = conn.query_row(
        "SELECT customer_id, entry_type FROM customer_payments WHERE id = ?1",
        rusqlite::params![entry_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        format!("Ledger entry not found: {}", e)
    })?;

    if let Err(e) = conn.execute(
        "DELETE FROM customer_payments WHERE id = ?1",
        rusqlite::params![entry_id],
    ) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to delete entry: {}", e));
    }

    // Recompute customer's outstanding_balance from remaining entries
    recompute_customer_balance(&conn, customer_id)?;

    conn.execute("COMMIT", []).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        e.to_string()
    })?;

    Ok(())
}

/// Helper: recompute a customer's outstanding_balance from scratch.
///
/// outstanding_balance = SUM(sales.balance) + SUM(customer_payments amounts)
/// where customer_payments amounts are:
///   - 'payment'        → -amount (reduces balance)
///   - 'opening_debit'  → +amount (increases balance)
///   - 'adjustment'     → +amount (signed; can be negative)
///
/// This is called after update/delete of a ledger entry to ensure the
/// denormalized outstanding_balance column always matches the source rows.
fn recompute_customer_balance(conn: &Connection, customer_id: i64) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();

    // Sum of all sale balances for this customer
    let sales_total: f64 = conn.query_row(
        "SELECT COALESCE(SUM(balance), 0.0) FROM sales WHERE customer_id = ?1",
        rusqlite::params![customer_id],
        |r| r.get(0),
    ).map_err(|e| format!("Failed to sum sales: {}", e))?;

    // Sum of all customer_payments with sign based on entry_type
    let payments_net: f64 = conn.query_row(
        "SELECT COALESCE(SUM(
            CASE WHEN entry_type = 'payment' THEN -amount
                 WHEN entry_type = 'opening_debit' THEN amount
                 WHEN entry_type = 'adjustment' THEN amount
                 ELSE 0.0
            END
         ), 0.0)
         FROM customer_payments WHERE customer_id = ?1",
        rusqlite::params![customer_id],
        |r| r.get(0),
    ).map_err(|e| format!("Failed to sum payments: {}", e))?;

    let new_balance = sales_total + payments_net;
    // Balance should never go negative (safety clamp)
    let new_balance = if new_balance < 0.0 { 0.0 } else { new_balance };

    conn.execute(
        "UPDATE customers SET outstanding_balance = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![new_balance, &now, customer_id],
    ).map_err(|e| format!("Failed to update balance: {}", e))?;

    Ok(())
}
