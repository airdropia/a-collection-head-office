use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::fs;
use tokio::time::{sleep, Duration};
use tauri::Emitter;

pub fn start_scheduler(db_path: PathBuf, app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        runtime.block_on(async move {
            loop {
                if let Ok(conn) = Connection::open(&db_path) {
                    let _ = run_due_automations(&conn, &db_path, &app_handle);
                }
                sleep(Duration::from_secs(3600)).await;
            }
        });
    });
}

fn run_due_automations(conn: &Connection, db_path: &Path, app_handle: &tauri::AppHandle) -> Result<(), String> {
    // 1. Database Backup automation check (DB-only, daily)
    if is_automation_due(conn, "Database Backup", 1)? {
        if let Ok(backup_path) = get_setting(conn, "backup_path") {
            if !backup_path.is_empty() {
                let backup_dir = Path::new(&backup_path);
                if backup_dir.exists() {
                    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                    let dest = backup_dir.join(format!("collection_ho_backup_{}.db", timestamp));
                    fs::copy(db_path, dest).map_err(|e| e.to_string())?;
                    update_automation_last_run(conn, "Database Backup")?;
                    let _ = app_handle.emit("automation-run", "Database Backup Successful");

                    // v0.22.0: Also create a FULL backup (DB + images + settings) as ZIP.
                    // This is Ali bhai's "daily full backup" requirement — if AppData is
                    // accidentally deleted, this ZIP allows one-click restore.
                    let _ = create_full_backup_in_dir(conn, db_path, backup_dir);
                }
            }
        }
    }

    // 2. Weekly Performance Report check
    if is_automation_due(conn, "Weekly Performance Report", 7)? {
        if let Ok(backup_path) = get_setting(conn, "backup_path") {
            if !backup_path.is_empty() {
                let backup_dir = Path::new(&backup_path);
                if backup_dir.exists() {
                    let timestamp = chrono::Local::now().format("%Y%m%d").to_string();
                    let dest = backup_dir.join(format!("weekly_report_{}.txt", timestamp));

                    // Generate a quick text report
                    if let Ok(report_text) = compile_weekly_summary(conn) {
                        fs::write(dest, report_text).map_err(|e| e.to_string())?;
                        update_automation_last_run(conn, "Weekly Performance Report")?;
                        let _ = app_handle.emit("automation-run", "Weekly Performance Report Created");
                    }
                }
            }
        }
    }

    Ok(())
}

/// v0.22.0: Create a FULL backup (DB + images + settings) as ZIP.
/// Called daily alongside DB-only backup. Silent — errors are logged via emit.
fn create_full_backup_in_dir(conn: &Connection, db_path: &Path, backup_dir: &Path) -> Result<(), String> {
    use std::io::Write;
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let zip_filename = format!("full_backup_{}.zip", timestamp);
    let zip_path = backup_dir.join(&zip_filename);

    // Export settings as JSON
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings")
        .map_err(|e| format!("Failed to prepare settings query: {}", e))?;
    let all_settings: std::collections::HashMap<String, String> = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("Failed to read settings: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    let settings_json = serde_json::to_string_pretty(&all_settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    // Build ZIP
    let zip_file = fs::File::create(&zip_path).map_err(|e| format!("Create zip: {}", e))?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Add database.db
    if db_path.exists() {
        let db_bytes = fs::read(db_path).map_err(|e| format!("Read DB: {}", e))?;
        zip.start_file("database.db", opts).map_err(|e| format!("Add DB to zip: {}", e))?;
        zip.write_all(&db_bytes).map_err(|e| format!("Write DB to zip: {}", e))?;
    }

    // Add settings.json
    zip.start_file("settings.json", opts).map_err(|e| format!("Add settings: {}", e))?;
    zip.write_all(settings_json.as_bytes()).map_err(|e| format!("Write settings: {}", e))?;

    // Add all images from AppData/images/
    let images_dir = crate::utils::get_images_dir();
    if images_dir.exists() {
        let image_files: Vec<_> = fs::read_dir(&images_dir)
            .map_err(|e| format!("Read images dir: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .collect();

        for img_file in image_files {
            let img_path = img_file.path();
            if let Some(img_name) = img_path.file_name().and_then(|n| n.to_str()) {
                let img_bytes = fs::read(&img_path).map_err(|e| format!("Read image {}: {}", img_name, e))?;
                let zip_name = format!("images/{}", img_name);
                zip.start_file(&zip_name, opts).map_err(|e| format!("Add image: {}", e))?;
                zip.write_all(&img_bytes).map_err(|e| format!("Write image: {}", e))?;
            }
        }
    }

    zip.finish().map_err(|e| format!("Finalize zip: {}", e))?;
    Ok(())
}

fn get_setting(conn: &Connection, key: &str) -> Result<String, String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [key],
        |row| row.get(0),
    ).map_err(|e| e.to_string())
}

fn is_automation_due(conn: &Connection, name: &str, interval_days: i64) -> Result<bool, String> {
    let row: Option<(String, i64)> = conn.query_row(
        "SELECT last_run, active FROM automations WHERE name = ?1",
        [name],
        |row| {
            let last_run: Option<String> = row.get(0)?;
            let active: i64 = row.get(1)?;
            Ok((last_run.unwrap_or_default(), active))
        },
    ).ok();

    if let Some((last_run, active)) = row {
        if active == 0 {
            return Ok(false);
        }
        if last_run.is_empty() {
            return Ok(true);
        }
        if let Ok(last_run_time) = chrono::DateTime::parse_from_rfc3339(&last_run) {
            let duration = chrono::Utc::now().signed_duration_since(last_run_time.with_timezone(&chrono::Utc));
            return Ok(duration.num_days() >= interval_days);
        }
    }
    Ok(false)
}

fn update_automation_last_run(conn: &Connection, name: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE automations SET last_run = ?1 WHERE name = ?2",
        params![now, name],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn compile_weekly_summary(conn: &Connection) -> Result<String, String> {
    let last_week = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();

    let (total_orders, sales, profit): (i64, f64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(total_amount), 0.0), COALESCE(SUM(profit), 0.0)
         FROM orders WHERE order_date >= ?1",
        [&last_week],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).map_err(|e| e.to_string())?;

    let low_stock_count: i64 = conn.query_row(
        // v0.35.0: split-columns rule (HO + agents stock), legacy fallback kept
        "SELECT COUNT(*) FROM products WHERE (COALESCE(qty_in_head_office, stock_quantity, 0) + COALESCE(qty_with_agents, 0)) <= 5 AND COALESCE(profit_status, 'in_head_office') != 'sold_out' AND status = 'active'",
        [],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    // Read currency from business_profile. Reuse the existing public getter
    // so the weekly report uses the same currency as the AI business context.
    // Previously hardcoded as "${:.2}" which was incorrect for PKR business.
    let currency = {
        let profile = crate::ai::get_business_profile(conn).unwrap_or_default();
        profile["currency"].as_str().unwrap_or("PKR").to_string()
    };

    let report = format!(
        "=========================================\n\
         WEEKLY BUSINESS SUMMARY REPORT\n\
         Date: {}\n\
         =========================================\n\n\
         Sales Activity (Last 7 Days):\n\
         - Total Orders: {}\n\
         - Gross Sales: {}\n\
         - Total Profit: {}\n\n\
         Inventory Health:\n\
         - Low Stock Items: {}\n\n\
         Generated automatically by A Collection Head Office Operating System.\n\
         =========================================",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        total_orders,
        crate::utils::format_money(sales, &currency),
        crate::utils::format_money(profit, &currency),
        low_stock_count
    );

    Ok(report)
}
