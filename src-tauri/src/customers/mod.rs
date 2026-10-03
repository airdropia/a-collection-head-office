use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

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
    // v0.44.0: dual-bucket khata — green (lena hai) and red (dena hai)
    // shown SEPARATELY, per owner directive. Net = udhaar_gross - advance_gross.
    #[serde(default)]
    pub udhaar_gross: f64,
    #[serde(default)]
    pub advance_gross: f64,
    #[serde(default)]
    pub segment: Option<String>,
}

/// v0.26.0: A single entry in a customer's balance history.
/// Either a sale (increases balance) or a payment (decreases balance).
/// Used by the customer detail modal to show the full khata timeline.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BalanceHistoryEntry {
    pub id: i64,
    pub entry_type: String, // "sale" | "payment"
    pub date: String,
    pub description: String, // product name + qty, or payment notes
    pub amount: f64,         // positive for sale, negative for payment
    pub balance_after: f64,  // running balance after this entry
}

pub fn get_all_customers(conn: &Connection) -> Result<Vec<Customer>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, name, phone, location, notes, created_at, COALESCE(outstanding_balance, 0.0), COALESCE(udhaar_gross, 0.0), COALESCE(advance_gross, 0.0), COALESCE(segment, 'general')
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
            udhaar_gross: row.get(7)?,
            advance_gross: row.get(8)?,
            segment: row.get(9)?,
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
            customer.id,
        ),
    )?;
    Ok(())
}

pub fn delete_customer(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM customers WHERE id = ?1", params![id])?;
    Ok(())
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
    // v0.44.0: dual-bucket drift detail (net may match while buckets are
    // stale — e.g. the first launch after the upgrade backfills 0.0 caches)
    #[serde(default)]
    pub udhaar_stored: f64,
    #[serde(default)]
    pub udhaar_computed: f64,
    #[serde(default)]
    pub advance_stored: f64,
    #[serde(default)]
    pub advance_computed: f64,
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

// ============================================================
// v0.44.0 — DUAL-BUCKET KHATA (udhaar_gross / advance_gross)
// ============================================================
//
// Owner directive (2026-10-02): net-only display was confusing — green
// (lena hai) and red (dena hai) must BOTH be visible, per customer and
// overall. Buckets are derived from the SAME ledger source of truth as
// CANONICAL_OUTSTANDING_SQL via a waterfall walk:
//
//   udhaar_gross  (GREEN) = soot/cash we gave, still un-recovered
//   advance_gross (RED)   = advances/overpayments we hold, still un-settled
//   outstanding_balance    = udhaar_gross - advance_gross (net, semantics
//                           unchanged — equals the canonical aggregate)
//
// Waterfall rules (owner-confirmed):
//   * maal diya (sale balance B > 0): advance PEHLE khata hai
//     (red -= min(B, red)); bachi raqam udhaar banti hai (green += rest)
//   * paisa aaya (payment / negative adjustment P): udhaar PEHLE settle
//     (green -= min(P, green)); excess advance banta hai (red += rest)
//   * opening_debit / positive adjustment: green += amount
//   * overpaid sale (balance < 0): cash-in jaisa treat hota hai
//
// Math property: creation and cash-in events COMMUTE in this waterfall
// (verified algebraically), so the final buckets are order-independent —
// the chronological sort is for determinism only.

#[derive(Debug, Clone, Copy)]
pub struct CustomerBuckets {
    pub udhaar_gross: f64,
    pub advance_gross: f64,
}

fn bucket_cash_in(p: f64, green: &mut f64, red: &mut f64) {
    let applied = p.min(*green);
    *green -= applied;
    *red += p - applied;
}

/// Waterfall walk over one customer's ledger (sales + customer_payments).
/// Read-only. Net invariant: green - red == CANONICAL_OUTSTANDING_SQL
/// (identical event set + signs), so buckets can never disagree with the
/// canonical net beyond float noise.
pub fn compute_customer_buckets(
    conn: &Connection,
    customer_id: i64,
) -> Result<CustomerBuckets, String> {
    // events: (date, id, table_rank, kind, amount)
    // kind 0 = plain udhaar-creation (opening_debit / +adjustment)
    // kind 1 = cash-in (payment / -adjustment / overpaid sale)
    // kind 2 = sale-creation (consumes advance first, remainder udhaar)
    let mut events: Vec<(String, i64, i8, i8, f64)> = Vec::new();

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.sale_date, s.balance FROM sales s
         WHERE s.customer_id = ?1 AND COALESCE(s.reversed, 0) = 0",
        )
        .map_err(|e| e.to_string())?;
    let sale_rows = stmt
        .query_map(params![customer_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for r in sale_rows {
        let (id, date, balance) = r.map_err(|e| e.to_string())?;
        if balance > 0.0 {
            events.push((date, id, 0, 2, balance));
        } else if balance < 0.0 {
            events.push((date, id, 0, 1, -balance));
        }
    }

    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.payment_date, COALESCE(p.entry_type, 'payment'), p.amount
         FROM customer_payments p WHERE p.customer_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let pay_rows = stmt
        .query_map(params![customer_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for r in pay_rows {
        let (id, date, etype, amount) = r.map_err(|e| e.to_string())?;
        match etype.as_str() {
            "opening_debit" => events.push((date, id, 1, 0, amount)),
            "adjustment" => {
                if amount >= 0.0 {
                    events.push((date, id, 1, 0, amount));
                } else {
                    events.push((date, id, 1, 1, -amount));
                }
            }
            _ => events.push((date, id, 1, 1, amount)), // payment / legacy NULL
        }
    }

    events.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut green = 0.0_f64;
    let mut red = 0.0_f64;
    for (_, _, _, kind, amount) in events {
        match kind {
            0 => green += amount,
            1 => bucket_cash_in(amount, &mut green, &mut red),
            _ => {
                let consumed = amount.min(red);
                red -= consumed;
                green += amount - consumed;
            }
        }
    }
    Ok(CustomerBuckets {
        udhaar_gross: green,
        advance_gross: red,
    })
}

/// Rewrite the cached khata columns (net + both buckets) for one customer
/// from the ledger waterfall. MUST be called inside the same transaction
/// as the ledger mutation. Ledger tables are never modified.
pub fn recompute_customer_buckets(conn: &Connection, customer_id: i64) -> Result<(), String> {
    let b = compute_customer_buckets(conn, customer_id)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE customers SET outstanding_balance = ?1, udhaar_gross = ?2, advance_gross = ?3, updated_at = ?4 WHERE id = ?5",
        params![b.udhaar_gross - b.advance_gross, b.udhaar_gross, b.advance_gross, &now, customer_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Read-only: report every customer whose stored khata caches (net or
/// either bucket) differ from the computed ledger waterfall (|drift| > 0.004).
pub fn get_customer_balance_drift(conn: &Connection) -> Result<Vec<BalanceDrift>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT c.id, c.name, COALESCE(c.outstanding_balance, 0.0),
                COALESCE(c.udhaar_gross, 0.0), COALESCE(c.advance_gross, 0.0),
                ({}) AS computed
         FROM customers c",
        CANONICAL_OUTSTANDING_SQL
    ))?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, f64>(4)?,
            row.get::<_, f64>(5)?,
        ))
    })?;
    let mut candidates: Vec<(i64, String, f64, f64, f64, f64)> = Vec::new();
    for r in rows {
        candidates.push(r?);
    }
    drop(stmt);

    let mut drifts = Vec::new();
    for (cid, name, stored_net, stored_u, stored_a, computed_net) in candidates {
        let buckets = compute_customer_buckets(conn, cid).unwrap_or(CustomerBuckets {
            udhaar_gross: stored_u,
            advance_gross: stored_a,
        });
        let net_diff = (stored_net - computed_net).abs() > 0.004;
        let u_diff = (stored_u - buckets.udhaar_gross).abs() > 0.004;
        let a_diff = (stored_a - buckets.advance_gross).abs() > 0.004;
        if net_diff || u_diff || a_diff {
            drifts.push(BalanceDrift {
                customer_id: cid,
                name,
                stored: stored_net,
                computed: computed_net,
                fixed: false,
                udhaar_stored: stored_u,
                udhaar_computed: buckets.udhaar_gross,
                advance_stored: stored_a,
                advance_computed: buckets.advance_gross,
            });
        }
    }
    Ok(drifts)
}

/// Write: recompute every customer's khata caches (net + both buckets) from
/// the canonical ledger and rewrite drifted rows. Returns the list of
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
            "UPDATE customers SET outstanding_balance = ?1, udhaar_gross = ?2, advance_gross = ?3, updated_at = ?4 WHERE id = ?5",
            params![d.computed, d.udhaar_computed, d.advance_computed, &now, d.customer_id],
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

    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| e.to_string())?;

    let current_balance: f64 = conn
        .query_row(
            "SELECT COALESCE(outstanding_balance, 0.0) FROM customers WHERE id = ?1",
            params![customer_id],
            |r| r.get(0),
        )
        .map_err(|e| {
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

    // v0.44.0: net + both buckets rewritten from the ledger waterfall
    // (was: incremental outstanding_balance -= amount)
    if let Err(e) = recompute_customer_buckets(conn, customer_id) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to update khata buckets: {}", e));
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

    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| e.to_string())?;

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

    // v0.44.0: net + both buckets rewritten from the ledger waterfall
    // (was: incremental outstanding_balance += amount)
    if let Err(e) = recompute_customer_buckets(conn, customer_id) {
        let _ = conn.execute("ROLLBACK", []);
        return Err(format!("Failed to update khata buckets: {}", e));
    }

    conn.execute("COMMIT", []).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        e.to_string()
    })?;

    Ok(entry_id)
}
