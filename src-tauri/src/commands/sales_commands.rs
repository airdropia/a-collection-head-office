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
// // Sales: record_sale
// ============================================================

// ============================================================
// v0.12.5 — Sales Recording (Head Office records ALL sales)
// ============================================================

/// Record a direct HO sale. (v0.36.0: agent walk-in sales removed with the
/// Agents feature — sales.agent_id stays NULL for new rows; historical
/// agent sales remain undoable via undo_sale.)
///
/// Auto-updates:
/// - sales table entry created
/// - product HO stock reduced
/// - product.qty_sold increased
/// - product.profit_status auto-recalculated
#[tauri::command]
pub async fn record_sale(
    state: State<'_, DbState>,
    product_id: i64,
    qty: i64,
    unit_sale_price: f64,
    sale_channel: String,
    customer_name: Option<String>,
    customer_phone: Option<String>,
    notes: Option<String>,
    amount_paid: Option<f64>,
    customer_id: Option<i64>,
) -> Result<i64, String> {
    if qty <= 0 {
        return Err("Quantity must be positive.".to_string());
    }
    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();
    let total = qty as f64 * unit_sale_price;

    // v0.14.4: Wrap the entire sale-recording flow in BEGIN IMMEDIATE / COMMIT.
    // record_sale writes to 2-3 tables (agent_ledger_entries, products,
    // sales) and does a stock-availability check before mutating. Without
    // a transaction, a crash between any two of these writes leaves the DB
    // inconsistent — e.g., the sales row exists but the product's
    // qty_sold was never incremented, or the ledger entry exists but the
    // sales row doesn't. BEGIN IMMEDIATE acquires a RESERVED lock so the
    // whole "check stock → insert ledger → update product → insert sale →
    // update profit_status" sequence is atomic.
    conn.execute("BEGIN IMMEDIATE", []).map_err(|e| e.to_string())?;

    // Helper closure to rollback on error and convert rusqlite::Error to String.
    // Used for the early-return paths below.
    macro_rules! try_or_rollback {
        ($expr:expr) => {
            match $expr {
                Ok(v) => v,
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", []);
                    return Err(e);
                }
            }
        };
    }

    // Direct HO sale — validate HO has enough stock
    // (v0.36.0: agent branch removed — all sales are HO sales now)
    let ho_qty: i64 = match conn.query_row(
        "SELECT COALESCE(qty_in_head_office, stock_quantity, 0) FROM products WHERE id = ?1",
        rusqlite::params![product_id],
        |r| r.get(0),
    ) {
        Ok(v) => v,
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            return Err(format!("Product not found: {}", e));
        }
    };
    if ho_qty < qty {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!(
            "Insufficient stock in Head Office. Available: {}, requested: {}.",
            ho_qty, qty
        ));
    }
    // Reduce HO stock, increase sold
    try_or_rollback!(conn.execute(
        "UPDATE products SET qty_in_head_office = qty_in_head_office - ?1, stock_quantity = stock_quantity - ?2, qty_sold = qty_sold + ?3, updated_at = ?4 WHERE id = ?5",
        rusqlite::params![qty, qty, qty, &now, product_id],
    ).map_err(|e| e.to_string()));

    // Create sales table entry
    // v0.26.0: Track amount_paid + balance for udhar/credit feature.
    // If amount_paid is None, default to full payment (backward compat).
    let paid = amount_paid.unwrap_or(total).min(total);
    let balance = (total - paid).max(0.0);
    try_or_rollback!(conn.execute(
        "INSERT INTO sales (product_id, sale_channel, sale_type, agent_id, qty, unit_sale_price, total_sale_amount, amount_paid, balance, customer_id, customer_name, customer_phone, notes, sale_date, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        rusqlite::params![
            product_id,
            &sale_channel,
            "direct_sale",
            Option::<i64>::None,
            qty,
            unit_sale_price,
            total,
            paid,
            balance,
            customer_id,
            customer_name.as_deref().unwrap_or(""),
            customer_phone.as_deref().unwrap_or(""),
            notes.as_deref().unwrap_or(""),
            &now,
            &now,
            &now,
        ],
    ).map_err(|e| e.to_string()));
    let sale_id = conn.last_insert_rowid();

    // Auto-update profit_status based on remaining stock
    let (ho_qty, agent_qty): (i64, i64) = conn.query_row(
        "SELECT COALESCE(qty_in_head_office, 0), COALESCE(qty_with_agents, 0) FROM products WHERE id = ?1",
        rusqlite::params![product_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap_or((0, 0));
    let new_status = if ho_qty == 0 && agent_qty == 0 {
        "sold_out"
    } else if ho_qty == 0 && agent_qty > 0 {
        "with_agent"
    } else {
        "in_head_office"
    };
    try_or_rollback!(conn.execute(
        "UPDATE products SET profit_status = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![new_status, &now, product_id],
    ).map_err(|e| e.to_string()));

    // v0.26.0: If customer_id is provided and there's an unpaid balance,
    // increase the customer's outstanding_balance by the sale's balance.
    // This keeps the khata (credit) tracking in sync.
    if balance > 0.0 {
        if let Some(cid) = customer_id {
            try_or_rollback!(conn.execute(
                "UPDATE customers SET outstanding_balance = outstanding_balance + ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![balance, &now, cid],
            ).map_err(|e| e.to_string()));
        }
    }

    conn.execute("COMMIT", []).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        e.to_string()
    })?;

    Ok(sale_id)
}

// ============================================================
// v0.30.0: SALE UNDO + SOLD ITEMS MANAGEMENT
// ============================================================
//
// Bhai ki need: "catalog mein agar kisi suite ki sale galti se record ho
// jaye to undo karne ka option ho. Sold items catalog se hide ho jayein
// (local + GitHub Pages). Phir agar wapas/change ho to reactivate/delete
// bhi kar saken."
//
// Implementation:
//   - undo_sale: reverses everything record_sale did (stock + qty_sold +
//     customer balance + agent ledger), marks sale as 'reversed' (soft
//     delete — sale row kept for audit trail, status flag flipped).
//   - Sold items management is handled in frontend (filter by profit_status
//     = 'sold_out') + catalog_publish_commands (exclude sold_out from
//     publish). No new Rust commands needed for that.

/// Undo a recorded sale. Reverses all side effects of record_sale:
/// - Restores product stock (qty_in_head_office OR qty_with_agents depending on sale type)
/// - Reduces product.qty_sold by the sale qty
/// - Recalculates product.profit_status
/// - Reverses customer's outstanding_balance update (if there was an unpaid balance)
/// - Removes the agent_ledger_entries row created for agent sales
/// - Marks the sale row as 'reversed' (status kept for audit, not hard-deleted)
///
/// This is a SOFT UNDO — the sale row remains in the database with a
/// `reversed = 1` flag, so the audit trail is preserved. The sale will
/// not appear in reports/summaries (filtered out by reversed = 0).
///
/// Cannot undo a sale that:
/// - Has already been reversed
/// - Has customer payments recorded against it (those would need to be
///   deleted first — caller's responsibility)
#[tauri::command]
pub async fn undo_sale(
    state: State<'_, DbState>,
    sale_id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute("BEGIN IMMEDIATE", []).map_err(|e| e.to_string())?;

    // Fetch sale details
    let (product_id, qty, agent_id, customer_id, balance, sale_channel): (
        i64, i64, Option<i64>, Option<i64>, f64, String,
    ) = conn.query_row(
        "SELECT product_id, qty, agent_id, customer_id, COALESCE(balance, 0.0), sale_channel
         FROM sales WHERE id = ?1",
        rusqlite::params![sale_id],
        |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
        )),
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        format!("Sale not found: {}", e)
    })?;

    // Check if already reversed
    let already_reversed: bool = conn.query_row(
        "SELECT COALESCE(reversed, 0) FROM sales WHERE id = ?1",
        rusqlite::params![sale_id],
        |r| Ok(r.get::<_, i64>(0)? != 0),
    ).unwrap_or(false);
    if already_reversed {
        let _ = conn.execute("ROLLBACK", []);
        return Err("Sale has already been reversed.".to_string());
    }

    // Check if any customer payments are linked to this sale
    let linked_payments: i64 = conn.query_row(
        "SELECT COUNT(*) FROM customer_payments WHERE sale_id = ?1",
        rusqlite::params![sale_id],
        |r| r.get(0),
    ).unwrap_or(0);
    if linked_payments > 0 {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!(
            "Cannot undo sale: {} customer payment(s) are linked to it. Delete those payments first from the customer's Khata History.",
            linked_payments
        ));
    }

    // Reverse stock effects
    if agent_id.is_some() {
        // Was agent sale — restore agent stock, reduce sold
        if let Err(e) = conn.execute(
            "UPDATE products SET qty_with_agents = qty_with_agents + ?1, qty_sold = MAX(0, qty_sold - ?2), updated_at = ?3 WHERE id = ?4",
            rusqlite::params![qty, qty, &now, product_id],
        ) {
            let _ = conn.execute("ROLLBACK", []);
            return Err(format!("Failed to restore agent stock: {}", e));
        }
        // Remove the sale_reported ledger entry (find by matching attributes,
        // pick the most recent one). SQLite DELETE doesn't support LIMIT
        // directly, so we use a subquery to find the row id first.
        let unit_sale_price: f64 = conn.query_row(
            "SELECT unit_sale_price FROM sales WHERE id = ?1",
            rusqlite::params![sale_id],
            |r| r.get(0),
        ).unwrap_or(0.0);
        let amount = qty as f64 * unit_sale_price;
        let sale_date: String = conn.query_row(
            "SELECT sale_date FROM sales WHERE id = ?1",
            rusqlite::params![sale_id],
            |r| r.get(0),
        ).unwrap_or_default();
        let ledger_entry_id: Option<i64> = conn.query_row(
            "SELECT id FROM agent_ledger_entries
             WHERE agent_id = ?1 AND product_id = ?2 AND entry_type = 'sale_reported'
             AND qty = ?3 AND amount = ?4 AND entry_date = ?5
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![agent_id, product_id, qty, amount, &sale_date],
            |r| r.get(0),
        ).ok();
        if let Some(entry_id) = ledger_entry_id {
            if let Err(e) = conn.execute(
                "DELETE FROM agent_ledger_entries WHERE id = ?1",
                rusqlite::params![entry_id],
            ) {
                let _ = conn.execute("ROLLBACK", []);
                return Err(format!("Failed to remove agent ledger entry: {}", e));
            }
        }
    } else {
        // Was direct HO sale — restore HO stock, reduce sold
        if let Err(e) = conn.execute(
            "UPDATE products SET qty_in_head_office = qty_in_head_office + ?1, stock_quantity = stock_quantity + ?2, qty_sold = MAX(0, qty_sold - ?3), updated_at = ?4 WHERE id = ?5",
            rusqlite::params![qty, qty, qty, &now, product_id],
        ) {
            let _ = conn.execute("ROLLBACK", []);
            return Err(format!("Failed to restore HO stock: {}", e));
        }
    }

    // Reverse customer balance update (if there was an unpaid balance)
    if balance > 0.0 {
        if let Some(cid) = customer_id {
            if let Err(e) = conn.execute(
                "UPDATE customers SET outstanding_balance = MAX(0, outstanding_balance - ?1), updated_at = ?2 WHERE id = ?3",
                rusqlite::params![balance, &now, cid],
            ) {
                let _ = conn.execute("ROLLBACK", []);
                return Err(format!("Failed to reverse customer balance: {}", e));
            }
        }
    }

    // Recalculate product.profit_status
    let (ho_qty, agent_qty): (i64, i64) = conn.query_row(
        "SELECT COALESCE(qty_in_head_office, 0), COALESCE(qty_with_agents, 0) FROM products WHERE id = ?1",
        rusqlite::params![product_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap_or((0, 0));
    let new_status = if ho_qty == 0 && agent_qty == 0 {
        "sold_out"
    } else if ho_qty == 0 && agent_qty > 0 {
        "with_agent"
    } else {
        "in_head_office"
    };
    if let Err(e) = conn.execute(
        "UPDATE products SET profit_status = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![new_status, &now, product_id],
    ) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to update profit_status: {}", e));
    }

    // Mark sale as reversed (soft delete — keep row for audit)
    let undone_tag = format!(" [UNDONE {}]", now);
    if let Err(e) = conn.execute(
        "UPDATE sales SET reversed = 1, notes = COALESCE(notes, '') || ?3, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![&now, sale_id, &undone_tag],
    ) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to mark sale as reversed: {}", e));
    }

    conn.execute("COMMIT", []).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        e.to_string()
    })?;

    let _ = sale_channel; // suppress unused warning
    Ok(())
}

/// Reactivate a sold-out product. Sets profit_status to 'in_head_office'
/// and restores qty_in_head_office to 1 (or the specified qty). Useful
/// when a sold-out item comes back into inventory (return, exchange, etc.).
#[tauri::command]
pub async fn reactivate_sold_product(
    state: State<'_, DbState>,
    product_id: i64,
    restock_qty: Option<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();
    let qty = restock_qty.unwrap_or(1);

    if qty <= 0 {
        return Err("Restock quantity must be positive.".to_string());
    }

    conn.execute(
        "UPDATE products
         SET profit_status = 'in_head_office',
             qty_in_head_office = COALESCE(qty_in_head_office, 0) + ?1,
             stock_quantity = COALESCE(stock_quantity, 0) + ?1,
             updated_at = ?2
         WHERE id = ?3",
        rusqlite::params![qty, &now, product_id],
    ).map_err(|e| format!("Failed to reactivate product: {}", e))?;

    Ok(())
}

// ============================================================
// v0.39.0 — Dashboard: Recent Sales panel (pi suggestion #3)
// ============================================================

/// One row of the Dashboard "Recent Sales (last 10)" panel.
/// Includes reversed sales (flagged) so the operator sees the full
/// activity trail; GUI renders reversed rows struck-through/grey.
#[derive(serde::Serialize)]
pub struct RecentSaleRow {
    pub id: i64,
    pub sale_date: String,
    pub product: Option<String>,
    pub qty: i64,
    pub total_sale_amount: f64,
    pub sale_channel: String,
    pub customer_name: Option<String>,
    pub reversed: i64,
}

#[tauri::command]
pub async fn get_recent_sales(
    state: State<'_, DbState>,
    limit: Option<i64>,
) -> Result<Vec<RecentSaleRow>, String> {
    let conn = state.0.lock().await;
    let n = limit.unwrap_or(10).clamp(1, 100);
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.sale_date, p.name, s.qty, s.total_sale_amount,
                    s.sale_channel, s.customer_name, COALESCE(s.reversed, 0)
             FROM sales s LEFT JOIN products p ON p.id = s.product_id
             ORDER BY s.sale_date DESC, s.id DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([n], |r| {
            Ok(RecentSaleRow {
                id: r.get(0)?,
                sale_date: r.get(1)?,
                product: r.get(2)?,
                qty: r.get(3)?,
                total_sale_amount: r.get(4)?,
                sale_channel: r.get(5)?,
                customer_name: r.get(6)?,
                reversed: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}
