//! acollectionho — A Collection Head Office write-CLI (Phase B, v0.35.0)
//!
//! Sanctioned WRITE path for head-office operations. Reuses the EXACT business
//! logic from the Tauri app's command layer (extracted *_impl functions +
//! plain module functions) — single source of truth for sign conventions,
//! validations, and side effects. No logic is duplicated here.
//!
//! Reads stay in the Bun CLI (cli/ac.ts). This binary only writes + heals.
//!
//! Safety model:
//!   - Same SQLite DB as the GUI app (WAL). Writes take BEGIN IMMEDIATE and
//!     respect busy_timeout. Best practice: app band rakhein during writes.
//!   - init_db() is idempotent (CREATE IF NOT EXISTS + migrations), so this
//!     works on the shop machine where the GUI app manages the schema.
//!
//! Usage:
//!   acollectionho pay-customer <customer_id> <amount> [--notes N] [--sale ID]
//!   acollectionho manual-entry <customer_id> <opening_debit|adjustment> <amount> [--notes N] [--date D]
//!   acollectionho customer-add --name N [--phone P] [--location L] [--notes N]
//!   acollectionho customer-edit <id> [--name N] [--phone P] [--location L] [--notes N]
//!   acollectionho product-add <sku> <name> <cost> <sale> [qty] [--category C]
//!                              [--brand B] [--fabric F] [--color C] [--retail R]
//!                              [--purchase P] [--desc D]
//!   acollectionho stock-add <product_id> <qty> [--notes N]
//!   acollectionho product-delete <sku> [--yes]  (never-sold products only)
//!   acollectionho record-sale <product_id> <qty> <unit_price> [--channel C] [--customer-id ID]
//!   acollectionho undo-sale <sale_id>
//!   acollectionho publish-catalog [--notes N]  (publish public catalog PWA to GitHub)
//!   acollectionho db-drift          (read-only drift report)
//!   acollectionho db-fix            (recompute outstanding caches from ledger)
//!   acollectionho version
//!
//! Exit codes: 0 ok, 1 error, 2 usage.

use a_collection_head_office_lib::{catalog, catalog_publish, commands::sales_commands, customers, database, inventory, utils};
use rusqlite::Connection;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn die_usage(msg: &str) -> ! {
    eprintln!("ERROR: {}", msg);
    eprintln!("run `acollectionho` with no args for usage");
    std::process::exit(2);
}

fn print_usage() {
    println!(
        "acollectionho v{} — A Collection Head Office write-CLI (Phase B)\n\
\n\
Writes (reuse the GUI app's exact business logic):\n\
  pay-customer <customer_id> <amount> [--notes N] [--sale ID]\n\
  manual-entry <customer_id> <opening_debit|adjustment> <amount> [--notes N] [--date D]\n\
  customer-add --name N [--phone P] [--location L] [--notes N]\n\
  customer-edit <id> [--name N] [--phone P] [--location L] [--notes N]\n\
\n\
Products (v0.41.0 — new-maal entry, replaces removed Purchase Trips):\n\
  product-add <sku> <name> <cost> <sale> [qty]\n\
              [--category C] [--brand B] [--fabric F] [--color C]\n\
              [--retail R] [--purchase P] [--desc D]\n\
              same INSERT the GUI Catalog form runs; cost = khareed rate\n\
              (purchase_price defaults to it), qty lands in head office.\n\
  stock-add <product_id> <qty> [--notes N]\n\
              restock existing article (+/-; negative = correction).\n\
              moves qty_in_head_office in lockstep with stock_quantity.\n\
  product-delete <sku> [--yes]\n\
              remove a wrong/never-sold entry (smoke rows etc.).\n\
              BLOCKED if any sale rows exist (audit trail) or agents\n\
              hold stock; non-zero stock needs --yes confirm.\n\
\n\
Sales (v0.40.0 — the ONLY sales path, same logic as GUI):\n\
  record-sale <product_id> <qty> <unit_price> [--channel C] [--customer-id ID]\n\
              [--customer-name N] [--phone P] [--paid AMT] [--notes N]\n\
              channel default: head_office | paid default: full price.\n\
              --paid below total leaves udhar balance; with --customer-id\n\
              the khata updates automatically (same as GUI).\n\
  undo-sale <sale_id>         soft undo: stock restored, khata reversed,\n\
              sale row kept with reversed=1 (audit trail)\n\
\n\
Catalog (v0.39.0):\n\
  publish-catalog [--notes N]   build + upload public catalog PWA to GitHub\n\
                                (uses Settings > Catalog config + token;\n\
                                every attempt logs to catalog_publish_history)\n\
\n\
Balance heal (v0.35.0):\n\
  db-drift    report customers whose stored outstanding != canonical ledger aggregate\n\
  db-fix      rewrite drifted outstanding_balance caches (ledger tables untouched)\n\
\n\
  version\n\
\n\
Notes:\n\
  - Customer amounts follow khata sign conventions: payment reduces balance;\n\
    opening_debit must be positive; adjustment is signed (negative = advance).\n\
  - Best practice: GUI app band rakhein during writes.",
        VERSION
    );
}

/// Parse `--flag value` style options out of the args tail.
struct Opts {
    notes: Option<String>,
    sale_id: Option<i64>,
    date: Option<String>,
    name: Option<String>,
    phone: Option<String>,
    location: Option<String>,
    channel: Option<String>,
    customer_id: Option<i64>,
    paid: Option<f64>,
    // v0.41.0 — product-add flags
    category: Option<String>,
    brand: Option<String>,
    fabric: Option<String>,
    color: Option<String>,
    desc: Option<String>,
    retail: Option<f64>,
    purchase: Option<f64>,
    // v0.42.0 — product-delete confirm flag
    yes: bool,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts { notes: None, sale_id: None, date: None, name: None, phone: None, location: None, channel: None, customer_id: None, paid: None, category: None, brand: None, fabric: None, color: None, desc: None, retail: None, purchase: None, yes: false };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--yes" => {
                // v0.42.0: boolean flag — does NOT consume the next arg.
                o.yes = true;
            }
            "--notes" => {
                i += 1;
                o.notes = args.get(i).cloned();
            }
            "--sale" => {
                i += 1;
                o.sale_id = args.get(i).and_then(|v| v.parse().ok());
                if o.sale_id.is_none() {
                    die_usage("--sale needs a numeric sale id");
                }
            }
            "--date" => {
                i += 1;
                o.date = args.get(i).cloned();
            }
            "--name" => {
                i += 1;
                o.name = args.get(i).cloned();
            }
            "--phone" => {
                i += 1;
                o.phone = args.get(i).cloned();
            }
            "--location" => {
                i += 1;
                o.location = args.get(i).cloned();
            }
            "--channel" => {
                i += 1;
                o.channel = args.get(i).cloned();
            }
            "--customer-id" => {
                i += 1;
                o.customer_id = args.get(i).and_then(|v| v.parse().ok());
                if o.customer_id.is_none() {
                    die_usage("--customer-id needs a numeric customer id");
                }
            }
            "--paid" => {
                i += 1;
                o.paid = args.get(i).and_then(|v| v.parse().ok());
                if o.paid.is_none() {
                    die_usage("--paid needs a numeric amount");
                }
            }
            "--category" => {
                i += 1;
                o.category = args.get(i).cloned();
            }
            "--brand" => {
                i += 1;
                o.brand = args.get(i).cloned();
            }
            "--fabric" => {
                i += 1;
                o.fabric = args.get(i).cloned();
            }
            "--color" => {
                i += 1;
                o.color = args.get(i).cloned();
            }
            "--desc" => {
                i += 1;
                o.desc = args.get(i).cloned();
            }
            "--retail" => {
                i += 1;
                o.retail = args.get(i).and_then(|v| v.parse().ok());
                if o.retail.is_none() {
                    die_usage("--retail needs a numeric price");
                }
            }
            "--purchase" => {
                i += 1;
                o.purchase = args.get(i).and_then(|v| v.parse().ok());
                if o.purchase.is_none() {
                    die_usage("--purchase needs a numeric price");
                }
            }
            other => die_usage(&format!("unknown option '{}'", other)),
        }
        i += 1;
    }
    o
}

fn open_db() -> Connection {
    let db_path = utils::get_db_path();
    if !db_path.exists() {
        eprintln!(
            "ERROR: database not found at {}\n(app pehle kabhi start kiya hona chahiye — fresh DB banane ke liye GUI app chalayein)",
            db_path.display()
        );
        std::process::exit(1);
    }
    let conn = database::init_db(&db_path).expect("Failed to open database");
    // Wait up to 5s for a locked DB (GUI app mid-transaction) before failing.
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("failed to set busy_timeout");
    conn
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::SUCCESS;
    }

    let cmd = args[0].as_str();
    let rest = &args[1..];

    let result: Result<(), String> = (|| {
        match cmd {
            "version" => {
                println!("acollectionho v{} (app v{})", VERSION, VERSION);
                println!("db: {}", utils::get_db_path().display());
                Ok(())
            }

            "pay-customer" => {
                if rest.len() < 2 {
                    return Err("usage: pay-customer <customer_id> <amount> [--notes N] [--sale ID]".into());
                }
                let cid: i64 = rest[0].parse().map_err(|_| "customer_id must be numeric".to_string())?;
                let amount: f64 = rest[1].parse().map_err(|_| "amount must be numeric".to_string())?;
                let o = parse_opts(&rest[2..]);
                let conn = open_db();
                customers::record_payment_impl(&conn, cid, amount, o.notes.as_deref(), o.sale_id)?;
                println!("OK: payment Rs. {:.0} recorded for customer #{}", amount, cid);
                Ok(())
            }

            "manual-entry" => {
                if rest.len() < 3 {
                    return Err("usage: manual-entry <customer_id> <opening_debit|adjustment> <amount> [--notes N] [--date D]".into());
                }
                let cid: i64 = rest[0].parse().map_err(|_| "customer_id must be numeric".to_string())?;
                let etype = rest[1].clone();
                let amount: f64 = rest[2].parse().map_err(|_| "amount must be numeric (adjustment may be negative)".to_string())?;
                let o = parse_opts(&rest[3..]);
                let conn = open_db();
                let id = customers::add_manual_entry_impl(
                    &conn, cid, &etype, amount, o.notes.as_deref(), o.date.as_deref(),
                )?;
                println!("OK: {} entry Rs. {:.0} recorded for customer #{} (ledger id {})", etype, amount, cid, id);
                Ok(())
            }

            "customer-add" => {
                let o = parse_opts(rest);
                let name = o.name.clone().unwrap_or_default();
                if name.trim().is_empty() {
                    return Err("customer-add needs --name".into());
                }
                let c = customers::Customer {
                    id: None,
                    name: name.trim().to_string(),
                    phone: o.phone,
                    location: o.location,
                    notes: o.notes,
                    created_at: None,
                    outstanding_balance: 0.0,
                    segment: Some("general".to_string()),
                };
                let conn = open_db();
                let id = customers::add_customer(&conn, &c).map_err(|e| e.to_string())?;
                println!("OK: customer '{}' added (id {})", c.name, id);
                Ok(())
            }

            "customer-edit" => {
                if rest.is_empty() {
                    return Err("usage: customer-edit <id> [--name N] [--phone P] [--location L] [--notes N]".into());
                }
                let cid: i64 = rest[0].parse().map_err(|_| "customer_id must be numeric".to_string())?;
                let o = parse_opts(&rest[1..]);
                let conn = open_db();
                // load existing row
                let mut stmt = conn
                    .prepare("SELECT id, name, phone, location, notes, created_at, COALESCE(outstanding_balance,0.0), COALESCE(segment,'general') FROM customers WHERE id = ?1")
                    .map_err(|e| e.to_string())?;
                let mut c = stmt
                    .query_row([cid], |row| {
                        Ok(customers::Customer {
                            id: Some(row.get(0)?),
                            name: row.get(1)?,
                            phone: row.get(2)?,
                            location: row.get(3)?,
                            notes: row.get(4)?,
                            created_at: row.get(5)?,
                            outstanding_balance: row.get(6)?,
                            segment: row.get(7)?,
                        })
                    })
                    .map_err(|_| format!("customer #{} not found", cid))?;
                if let Some(n) = &o.name { c.name = n.trim().to_string(); }
                if o.phone.is_some() { c.phone = o.phone.clone(); }
                if o.location.is_some() { c.location = o.location.clone(); }
                if o.notes.is_some() { c.notes = o.notes.clone(); }
                customers::update_customer(&conn, &c).map_err(|e| e.to_string())?;
                println!("OK: customer #{} updated", cid);
                Ok(())
            }

            "product-add" => {
                // v0.41.0: new-maal entry path (Purchase Trips removed in
                // v0.38.0). Reuses catalog::add_product — the EXACT INSERT
                // the GUI Catalog form runs (stock_quantity + qty_in_head_office
                // lockstep, purchase_price cost basis, server-side timestamps).
                // Positionals stop at the first --flag, so flags can follow
                // in any order.
                let mut pos: Vec<&String> = Vec::new();
                let mut i = 0;
                while i < rest.len() && !rest[i].starts_with("--") {
                    pos.push(&rest[i]);
                    i += 1;
                }
                if pos.len() < 4 || pos.len() > 5 {
                    return Err("usage: product-add <sku> <name> <cost> <sale> [qty] [--category C] [--brand B] [--fabric F] [--color C] [--retail R] [--purchase P] [--desc D]".into());
                }
                let sku = pos[0].trim().to_string();
                let name = pos[1].trim().to_string();
                if sku.is_empty() || name.is_empty() {
                    return Err("sku and name must be non-empty".into());
                }
                let cost: f64 = pos[2].parse().map_err(|_| "cost must be numeric".to_string())?;
                let sale: f64 = pos[3].parse().map_err(|_| "sale must be numeric".to_string())?;
                if cost < 0.0 || sale < 0.0 {
                    return Err("cost and sale must be >= 0".into());
                }
                let qty: i64 = match pos.get(4) {
                    Some(v) => v.parse().map_err(|_| "qty must be numeric".to_string())?,
                    None => 0,
                };
                if qty < 0 {
                    return Err("qty must be >= 0 (stock deduction sirf record-sale/undo-sale se hoti hai)".into());
                }
                let o = parse_opts(&rest[i..]);
                let product = catalog::Product {
                    id: None,
                    sku: sku.clone(),
                    name: name.clone(),
                    category: o.category.clone(),
                    color: o.color.clone(),
                    design: None,
                    season: None,
                    cost_price: cost,
                    sale_price: sale,
                    purchase_price: o.purchase.unwrap_or(cost),
                    description: o.desc.clone(),
                    tags: None,
                    stock_quantity: qty,
                    status: "active".to_string(),
                    images: "[]".to_string(),
                    supplier_id: None,
                    created_at: None,
                    updated_at: None,
                    product_code: None,
                    brand: o.brand.clone(),
                    fabric: o.fabric.clone(),
                    size_info: None,
                    retail_price: o.retail,
                    discount_price: None,
                    qty_in_head_office: Some(qty),
                    qty_with_agents: Some(0),
                    qty_sold: Some(0),
                    qty_reserved: Some(0),
                    profit_status: Some("in_head_office".to_string()),
                };
                let conn = open_db();
                match catalog::add_product(&conn, &product) {
                    Ok(id) => {
                        println!("OK: product #{} added — {} ({})", id, name, sku);
                        println!("cost: Rs. {:.0} | sale: Rs. {:.0} | qty: {} (head office)", cost, sale, qty);
                        if let Some(r) = o.retail {
                            println!("retail (caption price): Rs. {:.0}", r);
                        }
                        println!("PWA par publish karne ke liye: publish-catalog");
                        Ok(())
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("UNIQUE") {
                            Err(format!("SKU '{}' already exists — duplicate article? (different SKU use karein ya existing product ko stock-add karein)", sku))
                        } else {
                            Err(msg)
                        }
                    }
                }
            }

            "stock-add" => {
                // v0.41.0: restock path — reuses inventory::adjust_stock (the
                // same fn the GUI Inventory tab runs). v0.41.0 impl hotfix
                // moves qty_in_head_office in lockstep with stock_quantity,
                // so Dashboard + Catalog figures stay correct.
                let mut pos: Vec<&String> = Vec::new();
                let mut i = 0;
                while i < rest.len() && !rest[i].starts_with("--") {
                    pos.push(&rest[i]);
                    i += 1;
                }
                if pos.len() != 2 {
                    return Err("usage: stock-add <product_id> <qty> [--notes N]  (negative qty = correction/deduct)".into());
                }
                let pid: i64 = pos[0].parse().map_err(|_| "product_id must be numeric".to_string())?;
                let qty: i64 = pos[1].parse().map_err(|_| "qty must be numeric".to_string())?;
                if qty == 0 {
                    return Err("qty must be non-zero".into());
                }
                let o = parse_opts(&rest[i..]);
                let conn = open_db();
                let before = catalog::get_product_by_id(&conn, pid)
                    .map_err(|_| format!("product #{} not found", pid))?;
                inventory::adjust_stock(&conn, pid, qty).map_err(|e| e.to_string())?;
                let after = catalog::get_product_by_id(&conn, pid).map_err(|e| e.to_string())?;
                let sign = if qty > 0 { "+" } else { "-" };
                println!(
                    "OK: product #{} '{}' ({}) stock {}{} -> {} (head office)",
                    pid, before.name, before.sku, sign, qty.abs(), after.stock_quantity
                );
                if let Some(n) = &o.notes {
                    if !n.trim().is_empty() {
                        println!("Notes: {}", n);
                    }
                }
                Ok(())
            }

            "product-delete" => {
                // v0.42.0: CLI removal path for wrong/never-sold entries
                // (smoke rows, galat darj). Guards BEFORE delete:
                //  - any sales rows (incl. reversed audit rows) -> BLOCKED;
                //    FK is ON DELETE RESTRICT anyway, and the audit trail is
                //    non-negotiable — no flag bypasses this
                //  - qty_with_agents > 0 -> BLOCKED (agent_ledger rows would
                //    SET NULL and unlink history)
                //  - stock != 0 -> requires --yes (units get freed)
                if rest.is_empty() {
                    return Err("usage: product-delete <sku> [--yes]".into());
                }
                let sku = rest[0].trim().to_string();
                if sku.starts_with("--") {
                    return Err("usage: product-delete <sku> [--yes]".into());
                }
                let o = parse_opts(&rest[1..]);
                let conn = open_db();
                let p = catalog::get_product_by_sku(&conn, &sku)
                    .map_err(|_| format!("no product with SKU '{}' — 'products list' se sahi SKU dekhein", sku))?;
                let pid = p.id.unwrap_or(0);
                let sales_rows: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM sales WHERE product_id = ?1",
                        [pid],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                if sales_rows > 0 {
                    return Err(format!(
                        "product '{}' (#{}): {} sale row(s) hain (audit trail) — delete BLOCKED. Becha hua article CLI se delete nahi hota.",
                        p.sku, pid, sales_rows
                    ));
                }
                let agents_qty = p.qty_with_agents.unwrap_or(0);
                if agents_qty > 0 {
                    return Err(format!(
                        "product '{}' (#{}): agents ke paas {} unit hain — pehle wapas karein; delete karne se agent ledger ka link toot jaye ga.",
                        p.sku, pid, agents_qty
                    ));
                }
                if p.stock_quantity != 0 && !o.yes {
                    return Err(format!(
                        "product '{}' (#{}): stock {} unit hai, delete par freed ho jaye gi. Confirm: product-delete {} --yes",
                        p.sku, pid, p.stock_quantity, p.sku
                    ));
                }
                catalog::delete_product(&conn, pid).map_err(|e| e.to_string())?;
                println!(
                    "OK: product #{} '{}' ({}) deleted — freed head-office stock: {}",
                    pid, p.name, p.sku, p.stock_quantity
                );
                Ok(())
            }

            "record-sale" => {
                // v0.40.0: CLI sales path — reuses record_sale_impl, the EXACT
                // logic the GUI sale modal runs (single source of truth).
                if rest.len() < 3 {
                    return Err("usage: record-sale <product_id> <qty> <unit_price> [--channel C] [--customer-id ID] [--customer-name N] [--phone P] [--paid AMT] [--notes N]".into());
                }
                let pid: i64 = rest[0].parse().map_err(|_| "product_id must be numeric".to_string())?;
                let qty: i64 = rest[1].parse().map_err(|_| "qty must be numeric".to_string())?;
                let price: f64 = rest[2].parse().map_err(|_| "unit_price must be numeric".to_string())?;
                let o = parse_opts(&rest[3..]);
                let conn = open_db();
                let sale_id = sales_commands::record_sale_impl(
                    &conn,
                    pid,
                    qty,
                    price,
                    o.channel.as_deref().unwrap_or("head_office"),
                    o.name.as_deref(),
                    o.phone.as_deref(),
                    o.notes.as_deref(),
                    o.paid,
                    o.customer_id,
                )?;
                let (total, paid, balance): (f64, f64, f64) = conn
                    .query_row(
                        "SELECT total_sale_amount, COALESCE(amount_paid, 0.0), COALESCE(balance, 0.0) FROM sales WHERE id = ?1",
                        [sale_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .map_err(|e| e.to_string())?;
                let channel_used = o.channel.as_deref().unwrap_or("head_office");
                println!("OK: sale #{} recorded — product #{} x{} @ Rs. {:.0} (channel: {})", sale_id, pid, qty, price, channel_used);
                println!("total: Rs. {:.0} | paid: Rs. {:.0} | udhar balance: Rs. {:.0}", total, paid, balance);
                if balance > 0.0 && o.customer_id.is_none() {
                    println!("NOTE: udhar balance NOT linked to any customer (no --customer-id) — khata untouched");
                }
                Ok(())
            }

            "undo-sale" => {
                // v0.40.0: CLI undo — reuses undo_sale_impl (agent branch kept
                // for historical agent sales per DATA POLICY).
                if rest.is_empty() {
                    return Err("usage: undo-sale <sale_id>".into());
                }
                let sid: i64 = rest[0].parse().map_err(|_| "sale_id must be numeric".to_string())?;
                let conn = open_db();
                sales_commands::undo_sale_impl(&conn, sid)?;
                println!("OK: sale #{} undone — stock restored, qty_sold reduced, khata reversed (if any)", sid);
                println!("sale row kept with reversed=1 (audit trail)");
                Ok(())
            }

            "publish-catalog" => {
                // v0.39.0: CLI publish path — reuses the EXACT catalog_publish
                // backend the GUI Settings > Catalog button uses (single source
                // of truth for validation, image naming, history logging).
                let o = parse_opts(rest);
                let conn = open_db();

                let get_setting = |key: &str| -> String {
                    conn.query_row(
                        "SELECT value FROM settings WHERE key = ?1",
                        [key],
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
                    return Err("GitHub token not configured — GUI app > Settings > Catalog mein token paste karein (catalog_github_token)".into());
                }

                // Build catalog + images while holding the connection (all sync).
                let mut catalog = catalog_publish::build_catalog_json(&conn, &brand, &whatsapp)
                    .map_err(|e| format!("Failed to build catalog: {}", e))?;
                let image_mapping = catalog_publish::generate_webp_images(&conn)
                    .map_err(|e| format!("Failed to generate images: {}", e))?;

                // v0.16.2 image-name rewrite (mirrors the GUI command exactly):
                // catalog.json must reference the uploaded catalog names.
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

                let (warnings_count, errors_count) = match
                    catalog_publish::build_preview(&conn, &brand, &whatsapp, &repo)
                {
                    Ok(p) => (p.warnings.len() as i64, p.errors.len() as i64),
                    Err(_) => (0, 0),
                };

                // Async GitHub upload — run on a local tokio runtime.
                let start_time = std::time::Instant::now();
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| format!("tokio runtime: {}", e))?;
                let result = rt.block_on(catalog_publish::upload_to_github(
                    &catalog, &image_mapping, &repo, &github_token,
                ));
                let duration_ms = start_time.elapsed().as_millis() as i64;

                // Log the attempt to catalog_publish_history (same as GUI).
                let (success, error_msg) = match &result {
                    Ok(r) => (r.success, if r.errors.is_empty() { None } else { Some(r.errors.join("; ")) }),
                    Err(e) => (false, Some(e.clone())),
                };
                let products_count = catalog.products.len() as i64;
                let images_uploaded = result.as_ref().map(|r| r.images_uploaded as i64).unwrap_or(0);
                let images_deleted = result.as_ref().map(|r| r.images_deleted as i64).unwrap_or(0);
                let catalog_version = catalog.version.clone();
                let _ = catalog_publish::log_publish_history(
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

                match result {
                    Ok(r) if r.success => {
                        println!("OK: catalog published — {} products, {} image(s) uploaded, {} deleted", r.products_published, r.images_uploaded, r.images_deleted);
                        if !r.catalog_url.is_empty() {
                            println!("URL: {}", r.catalog_url);
                        }
                        if let Some(n) = &o.notes {
                            if !n.trim().is_empty() {
                                println!("Notes: {}", n);
                            }
                        }
                        println!("Duration: {} ms | history logged (catalog_publish_history)", duration_ms);
                        Ok(())
                    }
                    Ok(r) => Err(format!("publish failed: {}", r.errors.join("; "))),
                    Err(e) => Err(e),
                }
            }

            "db-drift" => {
                let conn = open_db();
                let drifts = customers::get_customer_balance_drift(&conn).map_err(|e| e.to_string())?;
                if drifts.is_empty() {
                    println!("OK: no drift — all outstanding_balance caches match the canonical ledger aggregate");
                } else {
                    println!("DRIFT ({} rows):", drifts.len());
                    for d in &drifts {
                        println!("  #{} {} stored {} -> computed {}", d.customer_id, d.name, d.stored, d.computed);
                    }
                    println!("run `acollectionho db-fix` to rewrite the caches (ledger untouched)");
                }
                Ok(())
            }

            "db-fix" => {
                let conn = open_db();
                let fixed = customers::recompute_all_customer_balances(&conn).map_err(|e| e.to_string())?;
                if fixed.is_empty() {
                    println!("OK: nothing to fix");
                } else {
                    for d in &fixed {
                        println!("FIXED: #{} {} {} -> {}", d.customer_id, d.name, d.stored, d.computed);
                    }
                    println!("OK: {} cache row(s) rewritten from the ledger", fixed.len());
                }
                Ok(())
            }

            other => Err(format!(
                "unknown command '{}' — run `acollectionho` with no args for usage",
                other
            )),
        }
    })();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {}", e);
            ExitCode::FAILURE
        }
    }
}
