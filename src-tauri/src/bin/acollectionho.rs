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
//!   acollectionho publish-catalog [--notes N]  (publish public catalog PWA to GitHub)
//!   acollectionho db-drift          (read-only drift report)
//!   acollectionho db-fix            (recompute outstanding caches from ledger)
//!   acollectionho version
//!
//! Exit codes: 0 ok, 1 error, 2 usage.

use a_collection_head_office_lib::{catalog_publish, customers, database, utils};
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
}

fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts { notes: None, sale_id: None, date: None, name: None, phone: None, location: None };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
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
