use rusqlite::{Connection, Result};
use std::path::Path;
use std::fs;

pub fn init_db<P: AsRef<Path>>(db_path: P) -> Result<Connection> {
    if let Some(parent) = db_path.as_ref().parent() {
        fs::create_dir_all(parent).unwrap_or_default();
    }

    let mut conn = Connection::open(db_path)?;

    // v0.14.4: Enable WAL mode + tune PRAGMAs for concurrent read/write.
    // The automation scheduler writes backups + reports in the background
    // while the UI reads products — WAL lets these coexist without blocking.
    // synchronous=NORMAL is safe under WAL (writes are still durable to disk
    // on checkpoint). cache_size=-4096 means 4MB page cache (negative =
    // kibibytes). temp_store=MEMORY avoids temp files on disk for sorts.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -4096;
         PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = ON;"
    )?;

    run_migrations(&mut conn)?;

    // v0.35.0 (Phase B): auto-heal — recompute the customers.outstanding_balance
    // cache from the canonical ledger aggregate at every startup. The ledger
    // tables are NEVER touched; only drifted cache rows are rewritten.
    // Historical write paths (v0.26.x era edits) could leave the cache stale.
    match crate::customers::recompute_all_customer_balances(&mut conn) {
        Ok(drifts) if !drifts.is_empty() => {
            for d in &drifts {
                eprintln!(
                    "[auto-heal] customer #{} ({}) outstanding_balance {} -> {}",
                    d.customer_id, d.name, d.stored, d.computed
                );
            }
        }
        Ok(_) => {}
        Err(e) => eprintln!("[auto-heal] recompute failed (non-fatal): {}", e),
    }

    Ok(conn)
}

fn run_migrations(conn: &mut Connection) -> Result<()> {
    run_migrations_impl(conn)
}

/// Public wrapper around run_migrations so the `init_database` Tauri command
/// can trigger a re-sync of sales_areas -> locations without an app restart.
pub fn run_migrations_public(conn: &mut Connection) -> Result<()> {
    run_migrations_impl(conn)
}

fn run_migrations_impl(conn: &mut Connection) -> Result<()> {
    conn.execute("PRAGMA foreign_keys = ON;", [])?;

    // Existing tables (from before)
    conn.execute("CREATE TABLE IF NOT EXISTS products (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        sku TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        category TEXT,
        cost_price REAL NOT NULL,
        sale_price REAL NOT NULL,
        description TEXT,
        tags TEXT,
        stock_quantity INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL DEFAULT 'active',
        images TEXT NOT NULL DEFAULT '[]',
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );", [])?;

    conn.execute("CREATE TABLE IF NOT EXISTS customers (
        id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
        phone TEXT, location TEXT, notes TEXT, created_at TEXT NOT NULL
    );", [])?;

    conn.execute("CREATE TABLE IF NOT EXISTS orders (
        id INTEGER PRIMARY KEY AUTOINCREMENT, customer_id INTEGER NOT NULL,
        total_amount REAL NOT NULL, profit REAL NOT NULL, order_date TEXT NOT NULL,
        FOREIGN KEY(customer_id) REFERENCES customers(id) ON DELETE CASCADE
    );", [])?;

    conn.execute("CREATE TABLE IF NOT EXISTS order_items (
        id INTEGER PRIMARY KEY AUTOINCREMENT, order_id INTEGER NOT NULL,
        product_id INTEGER NOT NULL, quantity INTEGER NOT NULL,
        sale_price REAL NOT NULL, cost_price REAL NOT NULL,
        FOREIGN KEY(order_id) REFERENCES orders(id) ON DELETE CASCADE,
        FOREIGN KEY(product_id) REFERENCES products(id) ON DELETE RESTRICT
    );", [])?;

    // social_posts, ai_logs, ai_knowledge, business_memory, product_drafts,
    // media_assets — AI-era tables. CREATEs REMOVED in v0.37.0 (AI system
    // removed). Existing copies are dropped by the dead_tables list below.

    conn.execute("CREATE TABLE IF NOT EXISTS automations (
        id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE,
        schedule_type TEXT NOT NULL, last_run TEXT, active INTEGER NOT NULL DEFAULT 1
    );", [])?;

    conn.execute("CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY, value TEXT NOT NULL
    );", [])?;

    // === NEW TABLES (v0.4.0 — AI Workspace) ===
    // product_drafts / media_assets CREATEs REMOVED in v0.37.0 (AI removal).
    // v0.25.0's dead_tables drop below already removes them from live DBs.

    // === OLD NEW TABLES (v0.3.0) ===

    conn.execute("CREATE TABLE IF NOT EXISTS locations (
        id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE,
        address TEXT, is_active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL
    );", [])?;

    conn.execute("CREATE TABLE IF NOT EXISTS suppliers (
        id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
        contact TEXT, city TEXT, notes TEXT, created_at TEXT NOT NULL
    );", [])?;

    conn.execute("CREATE TABLE IF NOT EXISTS product_locations (
        id INTEGER PRIMARY KEY AUTOINCREMENT, product_id INTEGER NOT NULL,
        location_id INTEGER NOT NULL, quantity INTEGER NOT NULL DEFAULT 0,
        FOREIGN KEY(product_id) REFERENCES products(id) ON DELETE CASCADE,
        FOREIGN KEY(location_id) REFERENCES locations(id) ON DELETE CASCADE,
        UNIQUE(product_id, location_id)
    );", [])?;

    // === MIGRATIONS for existing columns ===
    add_col_if_missing(conn, "products", "product_code", "TEXT DEFAULT ''")?;
    add_col_if_missing(conn, "products", "color", "TEXT DEFAULT ''")?;
    add_col_if_missing(conn, "products", "design", "TEXT DEFAULT ''")?;
    add_col_if_missing(conn, "products", "season", "TEXT DEFAULT ''")?;
    add_col_if_missing(conn, "products", "supplier_id", "INTEGER DEFAULT NULL")?;
    add_col_if_missing(conn, "products", "purchase_price", "REAL DEFAULT 0.0")?;

    // ============================================================
    // v0.11.0 — Profit-Mode Refactor: Data Model Foundation
    // ============================================================
    // New tables for the profit-first operating system. Additive only —
    // no existing tables dropped, no existing data touched.

// purchase_trips / purchase_trip_items CREATEs REMOVED in v0.38.0
    // (feature removed; pi audit: 2 header-only trips, 0 items ever).
    // Existing tables are dropped by the dead_tables list below.

    // --- agents: replaces locations concept (person + place unified) ---
    // One agent = one person at a place. Existing locations data is migrated
    // to agents by sync_locations_to_agents() below.
    conn.execute("CREATE TABLE IF NOT EXISTS agents (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        agent_code TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        phone TEXT,
        city TEXT,
        area TEXT,
        address_notes TEXT,
        notes TEXT,
        is_active INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );", [])?;

    // --- agent_ledger_entries: unified stock + cash movement log ---
    // This is THE single source of truth for agent stock and money flow.
    // entry_type enum: stock_sent | stock_returned | sale_reported |
    //                  cash_received | balance_adjustment
    conn.execute("CREATE TABLE IF NOT EXISTS agent_ledger_entries (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        agent_id INTEGER NOT NULL,
        product_id INTEGER,
        entry_type TEXT NOT NULL,
        qty INTEGER NOT NULL DEFAULT 0,
        unit_price REAL NOT NULL DEFAULT 0.0,
        amount REAL NOT NULL DEFAULT 0.0,
        reference_code TEXT,
        notes TEXT,
        entry_date TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE,
        FOREIGN KEY(product_id) REFERENCES products(id) ON DELETE SET NULL
    );", [])?;

// share_logs CREATE REMOVED in v0.38.0 (Share Center removed;
    // pi audit: 0 rows EVER). Existing table dropped by dead_tables list.

    // v0.16.0: catalog_publish_history — log of every publish operation.
    // Useful for debugging and showing "last published" status in UI.
    conn.execute("CREATE TABLE IF NOT EXISTS catalog_publish_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        published_at TEXT NOT NULL,
        duration_ms INTEGER NOT NULL,
        products_count INTEGER NOT NULL,
        images_uploaded INTEGER NOT NULL,
        images_deleted INTEGER NOT NULL,
        success INTEGER NOT NULL,
        catalog_version TEXT,
        error_message TEXT,
        warnings_count INTEGER DEFAULT 0,
        errors_count INTEGER DEFAULT 0
    );", [])?;

    // --- sales: replaces orders table (single sales concept) ---
    // sale_channel enum: head_office | whatsapp | facebook | instagram |
    //                    tiktok | agent
    conn.execute("CREATE TABLE IF NOT EXISTS sales (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        product_id INTEGER NOT NULL,
        sale_channel TEXT NOT NULL DEFAULT 'head_office',
        sale_type TEXT,
        agent_id INTEGER,
        qty INTEGER NOT NULL DEFAULT 1,
        unit_sale_price REAL NOT NULL DEFAULT 0.0,
        total_sale_amount REAL NOT NULL DEFAULT 0.0,
        customer_name TEXT,
        customer_phone TEXT,
        notes TEXT,
        sale_date TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(product_id) REFERENCES products(id) ON DELETE RESTRICT,
        FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE SET NULL
    );", [])?;

    // --- products table: additive column extensions for profit-mode ---
    // Existing columns (sku, name, cost_price, sale_price, etc.) are kept
    // untouched. These new columns enable profit-mode features.
    add_col_if_missing(conn, "products", "source_trip_id", "INTEGER DEFAULT NULL")?;
    add_col_if_missing(conn, "products", "base_unit_cost", "REAL DEFAULT 0.0")?;
    add_col_if_missing(conn, "products", "landed_unit_cost", "REAL DEFAULT 0.0")?;
    add_col_if_missing(conn, "products", "retail_price", "REAL")?;
    add_col_if_missing(conn, "products", "discount_price", "REAL")?;
    add_col_if_missing(conn, "products", "size_info", "TEXT")?;
    add_col_if_missing(conn, "products", "brand", "TEXT")?;
    add_col_if_missing(conn, "products", "fabric", "TEXT")?;
    add_col_if_missing(conn, "products", "qty_with_agents", "INTEGER DEFAULT 0")?;
    add_col_if_missing(conn, "products", "qty_sold", "INTEGER DEFAULT 0")?;
    add_col_if_missing(conn, "products", "qty_reserved", "INTEGER DEFAULT 0")?;
    add_col_if_missing(conn, "products", "profit_status", "TEXT DEFAULT 'in_head_office'")?;
    // qty_in_head_office mirrors the legacy stock_quantity column but with
    // a clearer name in the profit-mode context. Backfilled from
    // stock_quantity on first migration; thereafter maintained by the
    // agent ledger functions (send_stock_to_agent, return_stock_from_agent).
    add_col_if_missing(conn, "products", "qty_in_head_office", "INTEGER DEFAULT 0")?;
    // Backfill qty_in_head_office from existing stock_quantity for legacy products.
    let _ = conn.execute(
        "UPDATE products SET qty_in_head_office = stock_quantity WHERE qty_in_head_office = 0 AND stock_quantity > 0",
        [],
    );
    // v0.14.3: REMOVED the `UPDATE products SET retail_price = sale_price`
    // backfill that used to run here. That backfill was the silent killer of
    // the "Save Rs. 0" bug — it overwrote every product's retail_price with
    // its sale_price, so ShareCenter always computed Save = retail − sale = 0.
    // Now retail_price stays NULL when not explicitly entered, and ShareCenter
    // falls back to `sale_price * 1.2` for the discount display.
    //
    // v0.14.3: ALSO undo the damage — clear retail_price wherever it equals
    // sale_price (the legacy backfill set them equal). This is idempotent:
    // after this UPDATE the values differ (retail_price = NULL, sale_price
    // unchanged), so it won't fire again on subsequent migrations. Products
    // the user has explicitly given a distinct retail_price are preserved.
    let _ = conn.execute(
        "UPDATE products SET retail_price = NULL WHERE retail_price IS NOT NULL AND retail_price = sale_price",
        [],
    );
    // Backfill base_unit_cost from existing purchase_price (or cost_price) for legacy products.
    let _ = conn.execute(
        "UPDATE products SET base_unit_cost = COALESCE(purchase_price, cost_price, 0.0) WHERE base_unit_cost = 0.0",
        [],
    );

    seed_initial_data(conn)?;
    ensure_business_profile(conn)?;
    // v0.12.7: locations table aur uske saare sync functions REMOVED.
    // Locations table DB mein exist karti hai (taake purana code break na
    // ho), lekin hum usme koi naya data insert nahi karte. Agents table
    // ab single source of truth hai.
    // seed_locations(conn)?;           // REMOVED — naye locations nahi banenge
    // sync_sales_areas_to_locations(conn)?;  // REMOVED
    // sync_locations_to_agents(conn)?;       // REMOVED
    // Sirf ek baar cleanup chalao taake purane duplicates (agar koi bache
    // hain) remove ho jayein.
    cleanup_duplicate_agents(conn)?;

    // ============================================================
    // v0.11.1 — Share Center enhancements
    // ============================================================
    // Add segment column to customers for bulk WhatsApp broadcasting.
    // segment values: 'women', 'girls', 'vip', 'agent', 'general', etc.
    // User-defined — stored as free-form text so they can create custom
    // segments beyond the defaults.
    add_col_if_missing(conn, "customers", "segment", "TEXT DEFAULT 'general'")?;
    add_col_if_missing(conn, "customers", "is_active", "INTEGER NOT NULL DEFAULT 1")?;
    // v0.26.1: Add updated_at column to customers table. The original
    // CREATE TABLE didn't include it, but v0.26.0's record_sale and
    // record_customer_payment commands reference it when updating
    // outstanding_balance. Without this column, those UPDATEs fail with
    // "no such column: updated_at".
    add_col_if_missing(conn, "customers", "updated_at", "TEXT")?;

    // v0.26.0: Udhar/Credit (khata) tracking.
    // customers.outstanding_balance = current total owed by this customer.
    //   Updated atomically on every sale (increases by balance amount) and
    //   every payment (decreases by payment amount).
    // sales.amount_paid = how much the customer paid at time of sale.
    //   Defaults to total_sale_amount (full payment) for backward compat.
    // sales.balance = total_sale_amount - amount_paid (carried for audit).
    // customer_payments table = each payment entry (date, amount, notes)
    //   so a customer's payment history is fully traceable + editable.
    add_col_if_missing(conn, "customers", "outstanding_balance", "REAL NOT NULL DEFAULT 0.0")?;
    add_col_if_missing(conn, "sales", "amount_paid", "REAL NOT NULL DEFAULT 0.0")?;
    add_col_if_missing(conn, "sales", "balance", "REAL NOT NULL DEFAULT 0.0")?;
    add_col_if_missing(conn, "sales", "customer_id", "INTEGER")?;
    // v0.30.0: reversed flag for soft-delete (undo_sale).
    // 0 = active sale (default)
    // 1 = reversed (sale row kept for audit, but excluded from reports/summaries)
    add_col_if_missing(conn, "sales", "reversed", "INTEGER NOT NULL DEFAULT 0")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS customer_payments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            customer_id INTEGER NOT NULL,
            amount REAL NOT NULL,
            payment_date TEXT NOT NULL,
            notes TEXT,
            sale_id INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(customer_id) REFERENCES customers(id) ON DELETE CASCADE,
            FOREIGN KEY(sale_id) REFERENCES sales(id) ON DELETE SET NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_customer_payments_customer
         ON customer_payments(customer_id, payment_date DESC)",
        [],
    )?;

    // v0.29.0: Add entry_type column to customer_payments.
    // Existing rows default to 'payment' (backward compatible).
    // New entry types:
    //   - 'payment'        (existing) customer paid against outstanding balance
    //   - 'opening_debit'  (new) legacy/old udhar added manually when onboarding
    //                       a customer whose previous balance wasn't tracked
    //   - 'adjustment'     (new) arbitrary correction (+/-). Positive amount
    //                       increases customer's outstanding_balance (they owe
    //                       more); negative decreases (discount/write-off).
    add_col_if_missing(conn, "customer_payments", "entry_type", "TEXT NOT NULL DEFAULT 'payment'")?;

    // v0.14.4: Add performance indexes.
    // agent_ledger_entries is queried heavily by get_agent_summary,
    // return_stock_from_agent, record_sale (all do SUM(CASE WHEN entry_type=...)
    // filtered by agent_id + product_id). With 1000+ ledger entries (realistic
    // after 6 months of business), these queries do full table scans. The
    // composite (agent_id, product_id) index covers the most common filter,
    // and (entry_type) speeds up the per-type SUMs.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ledger_agent_product
         ON agent_ledger_entries(agent_id, product_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ledger_entry_type
         ON agent_ledger_entries(entry_type)",
        [],
    )?;
    // products(status) is used by Catalog filters + dashboard stock queries.
    // Adding stock_quantity makes it a covering index for low-stock lookups.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_products_status
         ON products(status, stock_quantity)",
        [],
    )?;
    // v0.12.6: Clean up duplicate agents (same name, different agent_code)
    // that were created before the name-check fix in sync_locations_to_agents.
    cleanup_duplicate_agents(conn)?;

    // v0.25.0: Drop dead tables that were created in early versions but
    // never used. These tables consumed disk space and added schema noise
    // without serving any feature. DROP IF EXISTS is safe — if the table
    // doesn't exist (fresh install), the statement is a no-op.
    //   - business_memory: AI memory feature never wired up (v0.4.0)
    //   - product_drafts: AI drafts kept in Zustand state, not DB (v0.4.0)
    //   - media_assets: image metadata never stored here (v0.4.0)
    //   - suppliers: supplier UI never built (v0.1.0)
    //   - locations: replaced by agents in v0.11.0, sync function removed
    //   - product_locations: per-location stock never used (v0.1.0)
    // v0.37.0 additions — AI system fully removed (owner directive):
    //   - ai_logs: prompt/response logs, stale data (pi audit 2026-09-26)
    //   - ai_knowledge: AI knowledge base, feature removed
    //   - social_posts: zero rows EVER (pi audit 2026-09-26)
    let dead_tables = [
        "business_memory",
        "product_drafts",
        "media_assets",
        "suppliers",
        "product_locations",
        "locations",
        "ai_logs",
        "ai_knowledge",
        "social_posts",
        // v0.38.0 — Share Center + Purchase Trips removed (owner verdict,
        // pi audit 2026-09-26: share_logs 0 rows ever, trips 0 items):
        "share_logs",
        "purchase_trips",
        "purchase_trip_items",
    ];
    for table in &dead_tables {
        let _ = conn.execute(&format!("DROP TABLE IF EXISTS {}", table), []);
    }

    Ok(())
}

fn add_col_if_missing(conn: &Connection, table: &str, col: &str, col_def: &str) -> Result<()> {
    let exists: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name='{}'", table, col),
        [],
        |r| r.get(0),
    ).unwrap_or(0);
    if exists == 0 {
        conn.execute(&format!("ALTER TABLE {} ADD COLUMN {} {}", table, col, col_def), [])?;
    }
    Ok(())
}

const DEFAULT_PROFILE: &str = r#"{
  "business_name": "A Collection",
  "industry": "Ladies & Gents Clothing Retail",
  "currency": "PKR",
  "owner": "Ali",
  "purchase_city": "Faisalabad",
  "facebook_page": "https://www.facebook.com/profile.php?id=61589997236061",
  "whatsapp_channel": "https://whatsapp.com/channel/0029VbCcUycLNSaChZf2WJ2H",
  "whatsapp_number": "+923420830995",
  "sales_areas": ["Narowal", "Shakargarh", "Zafarwal", "Nearby Villages"],
  "sales_channels": ["Facebook", "WhatsApp", "Door To Door"],
  "collections": ["Summer", "Winter", "Eid Special", "Festive"],
  "target_customers": {
    "gender": "Female & Male",
    "income_group": "Middle Income",
    "preferred_products": [
      "3 Piece Suits", "2 Piece Suits", "Lawn", "Cotton",
      "Printed Designs", "Embroidery", "Cut Piece",
      "Gents Cotton", "Gents Washing Wear"
    ]
  },
  "business_goals": [
    "Increase Profit", "Increase Sales", "Reduce Dead Stock",
    "Improve Customer Retention", "Improve Marketing",
    "Auto-generate Social Media Posts"
  ],
  "assistant_roles": [
    "Inventory Manager", "Sales Analyst", "Marketing Assistant",
    "Business Advisor", "Purchase Planner", "Social Media Manager",
    "Product Photographer Assistant"
  ]
}"#;

fn ensure_business_profile(conn: &Connection) -> Result<()> {
    let val: Result<String, _> = conn.query_row(
        "SELECT value FROM settings WHERE key = 'business_profile'", [], |row| row.get(0),
    );
    match val {
        Ok(existing) => {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&existing) {
                if json.get("currency").is_none() || json.get("facebook_page").is_none() {
                    conn.execute("UPDATE settings SET value = ?1 WHERE key = 'business_profile'", [DEFAULT_PROFILE])?;
                }
            }
        }
        Err(_) => {
            conn.execute("INSERT OR IGNORE INTO settings (key, value) VALUES ('business_profile', ?1)", [DEFAULT_PROFILE])?;
        }
    }
    Ok(())
}

fn seed_locations(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM locations", [], |r| r.get(0)).unwrap_or(0);
    if count == 0 {
        let now = chrono::Utc::now().to_rfc3339();
        // Issue #6 fix: derive seed locations from DEFAULT_PROFILE.sales_areas
        // rather than hardcoding a separate 2-entry list. The business profile
        // already lists 4 sales_areas (Narowal, Shakargarh, Zafarwal, Nearby
        // Villages), and those should be the initial Location entries.
        if let Ok(profile) = serde_json::from_str::<serde_json::Value>(DEFAULT_PROFILE) {
            if let Some(areas) = profile["sales_areas"].as_array() {
                for area in areas {
                    if let Some(name) = area.as_str() {
                        let _ = conn.execute(
                            "INSERT OR IGNORE INTO locations (name, address, created_at) VALUES (?1, ?2, ?3)",
                            rusqlite::params![name, "", &now],
                        );
                    }
                }
                // Also add a "Head Office" entry as the operational base.
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO locations (name, address, created_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["Head Office", "Main Office", &now],
                );
                return Ok(());
            }
        }
        // Fallback if profile parsing failed for any reason
        let locs = [("Head Office", "Main Office"), ("Shakargarh Shop", "Shakargarh City")];
        for (name, addr) in &locs {
            conn.execute("INSERT INTO locations (name, address, created_at) VALUES (?1, ?2, ?3)", rusqlite::params![name, addr, &now])?;
        }
    }
    Ok(())
}

/// v0.12.6: Clean up duplicate agents that have the same name (case-insensitive).
/// Keeps the agent with the LOWEST id (first created), deletes the rest.
/// Ledger entries cascade-delete with the agent (ON DELETE CASCADE).
///
/// This runs on every startup but is a no-op if no duplicates exist.
fn cleanup_duplicate_agents(conn: &Connection) -> Result<()> {
    // Find all agent names that appear more than once (case-insensitive)
    let mut stmt = conn.prepare(
        "SELECT LOWER(name) AS lname, COUNT(*) as cnt
         FROM agents
         GROUP BY LOWER(name)
         HAVING cnt > 1"
    )?;
    let dupes: Vec<String> = stmt.query_map([], |row| {
        row.get::<_, String>(0)
    })?.filter_map(|r| r.ok()).collect();

    for lname in dupes {
        // Delete all agents with this name EXCEPT the one with the lowest id
        // (the first-created one). Ledger entries cascade-delete.
        conn.execute(
            "DELETE FROM agents WHERE LOWER(name) = ?1 AND id NOT IN (
                SELECT MIN(id) FROM agents WHERE LOWER(name) = ?1
            )",
            rusqlite::params![&lname],
        )?;
    }

    Ok(())
}

fn seed_initial_data(conn: &mut Connection) -> Result<()> {
    let settings_count: i64 = conn.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?;
    if settings_count == 0 {
        let default_settings = [
            ("theme", "dark"), ("ai_provider", "gemini"), ("ai_api_key", ""),
            ("ai_model", "gemini-2.0-flash"), ("backup_path", ""), ("backup_interval_days", "7"),
            ("business_profile", DEFAULT_PROFILE),
        ];
        for (k, v) in default_settings.iter() {
            conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2);", [k, v])?;
        }
    }

    let aut_count: i64 = conn.query_row("SELECT COUNT(*) FROM automations", [], |r| r.get(0))?;
    if aut_count == 0 {
        let automations = [("Database Backup", "daily"), ("Weekly Performance Report", "weekly"),
            ("Low Stock Reminder", "daily"), ("Dead Stock Audit", "monthly")];
        for (name, sched) in automations.iter() {
            conn.execute("INSERT INTO automations (name, schedule_type, active) VALUES (?1, ?2, 1);", [name, sched])?;
        }
    }

    // v0.14.5: REMOVED dummy product seeding entirely.
    // Previously, whenever the products table was empty (count == 0), this
    // block inserted 4 hardcoded products (Designer Linen Kurta, Casual
    // Cotton Shirt, Slim Fit Denim, Embroidered Lawn Suit). The user
    // reported that deleting these and restarting the app brought them
    // back — because the delete left the table empty, and the seed fired
    // again on next startup. Now the catalog starts genuinely empty on
    // fresh installs. The user creates every product themselves.
    //
    // NOTE: this also fixes the "weak delete system" perception — deletes
    // were working correctly, but the seeder was undoing them on restart.

    let cust_count: i64 = conn.query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))?;
    if cust_count == 0 {
        let now = chrono::Utc::now().to_rfc3339();
        let customers = [
            ("Ahmad Khan", "+923001234567", "Narowal, Pakistan", "Regular customer, prefers medium sizes."),
            ("Sara Ahmed", "+923219876543", "Shakargarh, Pakistan", "Interested in lawn collections."),
        ];
        for (name, phone, loc, notes) in customers.iter() {
            conn.execute("INSERT INTO customers (name, phone, location, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5);", (name, phone, loc, notes, &now))?;
        }
    }

    Ok(())
}
