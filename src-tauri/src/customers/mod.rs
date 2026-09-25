use serde::{Serialize, Deserialize};
use rusqlite::{Connection, params};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Customer {
    pub id: Option<i64>,
    pub name: String,
    pub phone: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub outstanding_balance: f64,
    #[serde(default)]
    pub segment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OrderItemInput {
    pub product_id: i64,
    pub quantity: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OrderHistory {
    pub order_id: i64,
    pub order_date: String,
    pub total_amount: f64,
    pub profit: f64,
    pub items: Vec<OrderItemDetail>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OrderItemDetail {
    pub product_name: String,
    pub sku: String,
    pub quantity: i64,
    pub sale_price: f64,
}

/// v0.26.0: A single entry in a customer's balance history.
/// Either a sale (increases balance) or a payment (decreases balance).
/// Used by the customer detail modal to show the full khata timeline.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BalanceHistoryEntry {
    pub id: i64,
    pub entry_type: String,   // "sale" | "payment"
    pub date: String,
    pub description: String,  // product name + qty, or payment notes
    pub amount: f64,          // positive for sale, negative for payment
    pub balance_after: f64,   // running balance after this entry
}

pub fn get_all_customers(conn: &Connection) -> Result<Vec<Customer>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, name, phone, location, notes, created_at, COALESCE(outstanding_balance, 0.0), COALESCE(segment, 'general')
         FROM customers ORDER BY name ASC"
    )?;
    
    let customer_iter = stmt.query_map([], |row| {
        Ok(Customer {
            id: Some(row.get(0)?),
            name: row.get(1)?,
            phone: row.get(2)?,
            location: row.get(3)?,
            notes: row.get(4)?,
            created_at: Some(row.get(5)?),
            outstanding_balance: row.get(6)?,
            segment: row.get(7)?,
        })
    })?;

    let mut customers = Vec::new();
    for customer in customer_iter {
        customers.push(customer?);
    }
    Ok(customers)
}

pub fn add_customer(conn: &Connection, customer: &Customer) -> Result<i64, rusqlite::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO customers (name, phone, location, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            &customer.name,
            &customer.phone,
            &customer.location,
            &customer.notes,
            &now
        ),
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_customer(conn: &Connection, customer: &Customer) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE customers SET name = ?1, phone = ?2, location = ?3, notes = ?4 WHERE id = ?5",
        (
            &customer.name,
            &customer.phone,
            &customer.location,
            &customer.notes,
            customer.id
        ),
    )?;
    Ok(())
}

pub fn delete_customer(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM customers WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn create_order(conn: &mut Connection, customer_id: i64, items: Vec<OrderItemInput>) -> Result<i64, Box<dyn std::error::Error>> {
    let tx = conn.transaction()?;
    
    let mut total_amount = 0.0;
    let mut total_cost = 0.0;
    let now = chrono::Utc::now().to_rfc3339();
    
    // 1. Insert Order placeholder (will update later with correct totals)
    tx.execute(
        "INSERT INTO orders (customer_id, total_amount, profit, order_date) VALUES (?1, 0.0, 0.0, ?2)",
        params![customer_id, &now],
    )?;
    let order_id = tx.last_insert_rowid();

    // 2. Loop items, calculate prices, decrease stock
    for item in items {
        let (cost_price, sale_price, stock_qty): (f64, f64, i64) = tx.query_row(
            "SELECT cost_price, sale_price, stock_quantity FROM products WHERE id = ?1",
            params![item.product_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        
        if stock_qty < item.quantity {
            return Err(format!("Insufficient stock for product ID: {}", item.product_id).into());
        }
        
        // Update product stock
        tx.execute(
            "UPDATE products SET stock_quantity = stock_quantity - ?1, updated_at = ?2 WHERE id = ?3",
            params![item.quantity, &now, item.product_id],
        )?;

        // Insert order item
        tx.execute(
            "INSERT INTO order_items (order_id, product_id, quantity, sale_price, cost_price) 
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                order_id,
                item.product_id,
                item.quantity,
                sale_price,
                cost_price
            ],
        )?;

        total_amount += sale_price * (item.quantity as f64);
        total_cost += cost_price * (item.quantity as f64);
    }

    let profit = total_amount - total_cost;

    // 3. Update order with actual totals
    tx.execute(
        "UPDATE orders SET total_amount = ?1, profit = ?2 WHERE id = ?3",
        params![total_amount, profit, order_id],
    )?;

    tx.commit()?;
    Ok(order_id)
}

pub fn get_customer_purchase_history(conn: &Connection, customer_id: i64) -> Result<Vec<OrderHistory>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, order_date, total_amount, profit FROM orders WHERE customer_id = ?1 ORDER BY order_date DESC"
    )?;

    let order_iter = stmt.query_map(params![customer_id], |row| {
        let order_id: i64 = row.get(0)?;
        let order_date: String = row.get(1)?;
        let total_amount: f64 = row.get(2)?;
        let profit: f64 = row.get(3)?;
        Ok((order_id, order_date, total_amount, profit))
    })?;

    let mut history = Vec::new();
    for row in order_iter {
        let (order_id, order_date, total_amount, profit) = row?;
        
        // Get items for this order
        let mut item_stmt = conn.prepare(
            "SELECT p.name, p.sku, oi.quantity, oi.sale_price 
             FROM order_items oi
             JOIN products p ON oi.product_id = p.id
             WHERE oi.order_id = ?1"
        )?;
        
        let item_iter = item_stmt.query_map(params![order_id], |i_row| {
            Ok(OrderItemDetail {
                product_name: i_row.get(0)?,
                sku: i_row.get(1)?,
                quantity: i_row.get(2)?,
                sale_price: i_row.get(3)?,
            })
        })?;
        
        let mut items = Vec::new();
        for item in item_iter {
            items.push(item?);
        }

        history.push(OrderHistory {
            order_id,
            order_date,
            total_amount,
            profit,
            items,
        });
    }

    Ok(history)
}

// ============================================================
// v0.35.0 — Phase B: canonical balance recompute (auto-heal)
// ============================================================
//
// The authoritative customer outstanding is the aggregate of the FULL
// khata (mirrors get_customer_balance_history exactly):
//
//   computed = SUM(sales.balance WHERE reversed = 0)      (udhar sale debts)
//            - SUM(payments.amount)                       (entry_type 'payment' or NULL — legacy rows)
//            + SUM(opening_debit.amount)                  (always positive in DB)
//            + SUM(adjustment.amount)                     (signed in DB)
//
// The customers.outstanding_balance column is a maintained CACHE of this
// value, written by every write path (record_sale / undo_sale /
// record_customer_payment / add_customer_ledger_entry). Historical code
// paths could drift it; the recompute repairs the cache. It never touches
// the ledger tables — the ledger is the source of truth.

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BalanceDrift {
    pub customer_id: i64,
    pub name: String,
    pub stored: f64,
    pub computed: f64,
    pub fixed: bool,
}

pub const CANONICAL_OUTSTANDING_SQL: &str = "
  COALESCE((SELECT SUM(s.balance) FROM sales s
            WHERE s.customer_id = c.id AND COALESCE(s.reversed, 0) = 0), 0.0)
  - COALESCE((SELECT SUM(p.amount) FROM customer_payments p
              WHERE p.customer_id = c.id AND COALESCE(p.entry_type, 'payment') = 'payment'), 0.0)
  + COALESCE((SELECT SUM(p.amount) FROM customer_payments p
              WHERE p.customer_id = c.id AND COALESCE(p.entry_type, 'payment') = 'opening_debit'), 0.0)
  + COALESCE((SELECT SUM(p.amount) FROM customer_payments p
              WHERE p.customer_id = c.id AND COALESCE(p.entry_type, 'payment') = 'adjustment'), 0.0)";

/// Read-only: report every customer whose stored cache differs from the
/// computed canonical balance (|drift| > 0.004).
pub fn get_customer_balance_drift(conn: &Connection) -> Result<Vec<BalanceDrift>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT c.id, c.name, COALESCE(c.outstanding_balance, 0.0),
                ({}) AS computed
         FROM customers c",
        CANONICAL_OUTSTANDING_SQL
    ))?;
    let rows = stmt.query_map([], |row| {
        let stored: f64 = row.get(2)?;
        let computed: f64 = row.get(3)?;
        Ok(BalanceDrift {
            customer_id: row.get(0)?,
            name: row.get(1)?,
            stored,
            computed,
            fixed: false,
        })
    })?;
    let mut drifts = Vec::new();
    for r in rows {
        let d = r?;
        if (d.stored - d.computed).abs() > 0.004 {
            drifts.push(d);
        }
    }
    Ok(drifts)
}

/// Write: recompute every customer's outstanding_balance cache from the
/// canonical ledger aggregate and rewrite drifted rows. Returns the list of
/// rows that were out of sync (with `fixed = true` on the ones rewritten).
/// Ledger tables are NEVER modified by this function.
pub fn recompute_all_customer_balances(
    conn: &Connection,
) -> Result<Vec<BalanceDrift>, rusqlite::Error> {
    let drifts = get_customer_balance_drift(conn)?;
    if drifts.is_empty() {
        return Ok(drifts);
    }
    let now = chrono::Utc::now().to_rfc3339();
    for d in &drifts {
        conn.execute(
            "UPDATE customers SET outstanding_balance = ?1, updated_at = ?2 WHERE id = ?3",
            params![d.computed, &now, d.customer_id],
        )?;
    }
    Ok(drifts
        .into_iter()
        .map(|mut d| {
            d.fixed = true;
            d
        })
        .collect())
}

// ============================================================
// v0.35.0 — Phase B: extracted write-path impls (shared by the Tauri
// command layer AND the acollectionho write-CLI — single source of truth
// for business rules + sign conventions).
// ============================================================

/// Record a customer payment (reduces outstanding_balance).
/// Extracted verbatim from the record_customer_payment Tauri command.
pub fn record_payment_impl(
    conn: &Connection,
    customer_id: i64,
    amount: f64,
    notes: Option<&str>,
    sale_id: Option<i64>,
) -> Result<(), String> {
    if amount <= 0.0 {
        return Err("Payment amount must be positive.".to_string());
    }
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute("BEGIN IMMEDIATE", []).map_err(|e| e.to_string())?;

    let current_balance: f64 = conn.query_row(
        "SELECT COALESCE(outstanding_balance, 0.0) FROM customers WHERE id = ?1",
        params![customer_id],
        |r| r.get(0),
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        format!("Customer not found: {}", e)
    })?;

    if amount > current_balance {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!(
            "Payment (Rs. {:.0}) exceeds outstanding balance (Rs. {:.0}).",
            amount, current_balance
        ));
    }

    if let Err(e) = conn.execute(
        "INSERT INTO customer_payments (customer_id, amount, payment_date, notes, sale_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            customer_id,
            amount,
            &now,
            notes.unwrap_or(""),
            sale_id,
            &now,
            &now,
        ],
    ) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to insert payment: {}", e));
    }

    if let Err(e) = conn.execute(
        "UPDATE customers SET outstanding_balance = outstanding_balance - ?1, updated_at = ?2 WHERE id = ?3",
        params![amount, &now, customer_id],
    ) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to update balance: {}", e));
    }

    conn.execute("COMMIT", []).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        e.to_string()
    })?;

    Ok(())
}

/// Add a manual ledger entry (opening_debit | adjustment).
/// Extracted verbatim from the add_customer_ledger_entry Tauri command.
/// Sign convention: opening_debit amount must be > 0 (adds to balance);
/// adjustment amount is signed (negative reduces balance — e.g. advance).
pub fn add_manual_entry_impl(
    conn: &Connection,
    customer_id: i64,
    entry_type: &str,
    amount: f64,
    notes: Option<&str>,
    date: Option<&str>,
) -> Result<i64, String> {
    if entry_type != "opening_debit" && entry_type != "adjustment" {
        return Err(format!(
            "Invalid entry_type '{}'. Must be 'opening_debit' or 'adjustment'.",
            entry_type
        ));
    }
    if entry_type == "opening_debit" && amount <= 0.0 {
        return Err("Opening debit amount must be positive.".to_string());
    }
    // adjustment can be zero — no-op, but rejected for consistency with GUI
    if entry_type == "adjustment" && amount == 0.0 {
        return Err("Adjustment amount cannot be zero.".to_string());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let entry_date = date.unwrap_or(&now);

    conn.execute("BEGIN IMMEDIATE", []).map_err(|e| e.to_string())?;

    if conn
        .query_row(
            "SELECT id FROM customers WHERE id = ?1",
            params![customer_id],
            |r| r.get::<_, i64>(0),
        )
        .is_err()
    {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Customer not found: {}", customer_id));
    }

    let res = conn.execute(
        "INSERT INTO customer_payments (customer_id, amount, payment_date, notes, sale_id, entry_type, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7)",
        params![
            customer_id,
            amount,
            entry_date,
            notes.unwrap_or(""),
            entry_type,
            &now,
            &now,
        ],
    );
    if let Err(e) = res {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to insert ledger entry: {}", e));
    }
    let entry_id = conn.last_insert_rowid();

    if let Err(e) = conn.execute(
        "UPDATE customers SET outstanding_balance = outstanding_balance + ?1, updated_at = ?2 WHERE id = ?3",
        params![amount, &now, customer_id],
    ) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to update balance: {}", e));
    }

    conn.execute("COMMIT", []).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        e.to_string()
    })?;

    Ok(entry_id)
}
