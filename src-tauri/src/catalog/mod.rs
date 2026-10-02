use serde::{Serialize, Deserialize};
use rusqlite::{Connection, params};
use std::path::Path;
use std::fs;
use std::io::Cursor;
use image::{ImageReader, imageops::FilterType};
use csv::{ReaderBuilder, WriterBuilder};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Product {
    pub id: Option<i64>,
    pub sku: String,
    pub name: String,
    pub category: Option<String>,
    pub color: Option<String>,
    pub design: Option<String>,
    pub season: Option<String>,
    pub cost_price: f64,
    pub sale_price: f64,
    pub purchase_price: f64,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub stock_quantity: i64,
    pub status: String,
    pub images: String,
    pub supplier_id: Option<i64>,
    // v0.28.0: created_at + updated_at are now Optional (same pattern as
    // Customer.created_at in v0.26.1). Frontend add_product flow does not
    // send these fields (server generates them via chrono::Utc::now() inside
    // add_product/update_product). Required String caused "missing field
    // 'created_at'" deserialization error on add_product. None values from
    // frontend are simply ignored at INSERT time (server uses its own `now`).
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    // v0.11.0+ profit-mode fields (optional — old rows may not have them)
    #[serde(default)]
    pub product_code: Option<String>,
    #[serde(default)]
    pub brand: Option<String>,
    #[serde(default)]
    pub fabric: Option<String>,
    #[serde(default)]
    pub size_info: Option<String>,
    #[serde(default)]
    pub retail_price: Option<f64>,
    #[serde(default)]
    pub discount_price: Option<f64>,
    // v0.41.0: base_unit_cost / landed_unit_cost / source_trip_id REMOVED
    // from the struct (Purchase Trips leftovers — columns dropped by the
    // init_db dead-columns cleanup; nothing read or wrote them).
    #[serde(default)]
    pub qty_in_head_office: Option<i64>,
    #[serde(default)]
    pub qty_with_agents: Option<i64>,
    #[serde(default)]
    pub qty_sold: Option<i64>,
    #[serde(default)]
    pub qty_reserved: Option<i64>,
    #[serde(default)]
    pub profit_status: Option<String>,
}

pub fn get_all_products(conn: &Connection) -> Result<Vec<Product>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(sku,''), name, category, color, design, season,
                cost_price, sale_price, COALESCE(purchase_price, cost_price),
                description, tags, stock_quantity, status, images, supplier_id, created_at, updated_at,
                product_code, brand, fabric, size_info, retail_price, discount_price,
                qty_in_head_office, qty_with_agents, qty_sold, qty_reserved, profit_status
         FROM products ORDER BY id DESC"
    )?;
    let product_iter = stmt.query_map([], |row| {
        Ok(Product {
            id: Some(row.get(0)?),
            sku: row.get(1)?,
            name: row.get(2)?,
            category: row.get(3)?,
            color: row.get(4)?,
            design: row.get(5)?,
            season: row.get(6)?,
            cost_price: row.get(7)?,
            sale_price: row.get(8)?,
            purchase_price: row.get(9)?,
            description: row.get(10)?,
            tags: row.get(11)?,
            stock_quantity: row.get(12)?,
            status: row.get(13)?,
            images: row.get(14)?,
            supplier_id: row.get(15)?,
            created_at: Some(row.get(16)?),
            updated_at: Some(row.get(17)?),
            product_code: row.get(18)?,
            brand: row.get(19)?,
            fabric: row.get(20)?,
            size_info: row.get(21)?,
            retail_price: row.get(22)?,
            discount_price: row.get(23)?,
            qty_in_head_office: row.get(24)?,
            qty_with_agents: row.get(25)?,
            qty_sold: row.get(26)?,
            qty_reserved: row.get(27)?,
            profit_status: row.get(28)?,
        })
    })?;
    let mut products = Vec::new();
    for p in product_iter { products.push(p?); }
    Ok(products)
}

pub fn get_product_by_id(conn: &Connection, id: i64) -> Result<Product, rusqlite::Error> {
    conn.query_row(
        "SELECT id, COALESCE(sku,''), name, category, color, design, season,
                cost_price, sale_price, COALESCE(purchase_price, cost_price),
                description, tags, stock_quantity, status, images, supplier_id, created_at, updated_at,
                product_code, brand, fabric, size_info, retail_price, discount_price,
                qty_in_head_office, qty_with_agents, qty_sold, qty_reserved, profit_status
         FROM products WHERE id = ?1",
        [id],
        |row| {
            Ok(Product {
                id: Some(row.get(0)?),
                sku: row.get(1)?,
                name: row.get(2)?,
                category: row.get(3)?,
                color: row.get(4)?,
                design: row.get(5)?,
                season: row.get(6)?,
                cost_price: row.get(7)?,
                sale_price: row.get(8)?,
                purchase_price: row.get(9)?,
                description: row.get(10)?,
                tags: row.get(11)?,
                stock_quantity: row.get(12)?,
                status: row.get(13)?,
                images: row.get(14)?,
                supplier_id: row.get(15)?,
                created_at: Some(row.get(16)?),
                updated_at: Some(row.get(17)?),
                product_code: row.get(18)?,
                brand: row.get(19)?,
                fabric: row.get(20)?,
                size_info: row.get(21)?,
                retail_price: row.get(22)?,
                discount_price: row.get(23)?,
                qty_in_head_office: row.get(24)?,
                qty_with_agents: row.get(25)?,
                qty_sold: row.get(26)?,
                qty_reserved: row.get(27)?,
                profit_status: row.get(28)?,
            })
        },
    )
}

/// v0.42.0: resolve a product by its SKU (CLI product-delete entry point).
/// Thin wrapper — id lookup reuses get_product_by_id's full 29-col mapping.
pub fn get_product_by_sku(conn: &Connection, sku: &str) -> Result<Product, rusqlite::Error> {
    let id: i64 = conn.query_row(
        "SELECT id FROM products WHERE sku = ?1",
        [sku],
        |r| r.get(0),
    )?;
    get_product_by_id(conn, id)
}

pub fn add_product(conn: &Connection, product: &Product) -> Result<i64, rusqlite::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    // v0.14.3: Persist retail_price + brand + fabric to products table.
    // Previously these v0.11.0+ columns were silently dropped by the INSERT
    // (only 16 original columns written), so retail_price was never saved
    // — Catalog form fell back to writing it as a `_product_retail_<id>`
    // setting, which ShareCenter never read, so every product ended up with
    // retail_price = sale_price (due to the legacy migration backfill) and
    // every caption showed "Save Rs. 0!".
    //
    // v0.22.5: Also persist qty_in_head_office. Previously this column was
    // skipped on INSERT (defaulting to 0), and only the database migration
    // backfill could populate it from stock_quantity — which only fired when
    // qty_in_head_office was 0. The Catalog overview + Dashboard read
    // `qty_in_head_office ?? stock_quantity` (preferring qty_in_head_office),
    // so a freshly added product showed 0 stock until the next app restart.
    // Now both columns get the same value at INSERT time. ?12 is reused
    // (same product.stock_quantity value) — same pattern as ?16 for
    // created_at + updated_at.
    conn.execute(
        "INSERT INTO products (sku, name, category, color, design, season, cost_price, sale_price, purchase_price, description, tags, stock_quantity, qty_in_head_office, status, images, supplier_id, created_at, updated_at, retail_price, brand, fabric)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13, ?14, ?15, ?16, ?16, ?17, ?18, ?19)",
        rusqlite::params![
            &product.sku, &product.name, &product.category,
            &product.color, &product.design, &product.season,
            product.cost_price, product.sale_price, product.purchase_price,
            &product.description, &product.tags, product.stock_quantity,
            &product.status, &product.images, product.supplier_id, &now,
            product.retail_price, &product.brand, &product.fabric,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_product(conn: &Connection, product: &Product) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    // v0.14.3: Update retail_price + brand + fabric alongside the legacy
    // 16 columns. See add_product() comment for full context.
    //
    // v0.22.5: Also update qty_in_head_office in lockstep with
    // stock_quantity. The Catalog form's "Total Stock (Head Office)" box
    // writes to stock_quantity, but the Catalog overview + Dashboard read
    // `qty_in_head_office ?? stock_quantity` (preferring qty_in_head_office).
    // Without this sync, the legacy column got updated while the
    // profit-mode column stayed stale — UI showed the old value.
    // ?12 is reused (same product.stock_quantity value, no new param).
    conn.execute(
        "UPDATE products SET sku=?1, name=?2, category=?3, color=?4, design=?5, season=?6,
         cost_price=?7, sale_price=?8, purchase_price=?9, description=?10, tags=?11,
         stock_quantity=?12, qty_in_head_office=?12,
         status=?13, images=?14, supplier_id=?15, updated_at=?16,
         retail_price=?17, brand=?18, fabric=?19 WHERE id=?20",
        rusqlite::params![
            &product.sku, &product.name, &product.category,
            &product.color, &product.design, &product.season,
            product.cost_price, product.sale_price, product.purchase_price,
            &product.description, &product.tags, product.stock_quantity,
            &product.status, &product.images, product.supplier_id,
            &now,
            product.retail_price, &product.brand, &product.fabric,
            product.id,
        ],
    )?;
    Ok(())
}

pub fn delete_product(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    // v0.25.0: Removed DELETE FROM product_locations — table dropped in migration.
    conn.execute("DELETE FROM products WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn export_to_csv(conn: &Connection) -> Result<String, Box<dyn std::error::Error>> {
    let products = get_all_products(conn)?;
    let mut wtr = WriterBuilder::new().from_writer(vec![]);
    wtr.write_record(&["Product Code", "Name", "Category", "Color", "Design", "Season",
        "Cost Price", "Sale Price", "Description", "Tags", "Stock", "Status"])?;
    for p in products {
        wtr.write_record(&[
            p.sku, p.name, p.category.unwrap_or_default(),
            p.color.unwrap_or_default(), p.design.unwrap_or_default(),
            p.season.unwrap_or_default(), p.cost_price.to_string(),
            p.sale_price.to_string(), p.description.unwrap_or_default(),
            p.tags.unwrap_or_default(), p.stock_quantity.to_string(), p.status,
        ])?;
    }
    let data = String::from_utf8(wtr.into_inner()?)?;
    Ok(data)
}

pub fn import_from_csv(conn: &Connection, csv_content: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut rdr = ReaderBuilder::new().from_reader(csv_content.as_bytes());
    let now = chrono::Utc::now().to_rfc3339();
    for result in rdr.records() {
        let record = result?;
        if record.len() < 12 { continue; }
        conn.execute(
            "INSERT INTO products (sku, name, category, color, design, season, cost_price, sale_price, purchase_price, description, tags, stock_quantity, status, images, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?9, ?10, ?11, ?12, '[]', ?13, ?13)",
            rusqlite::params![&record[0], &record[1], &record[2], &record[3], &record[4], &record[5],
             record[6].parse::<f64>().unwrap_or(0.0), record[7].parse::<f64>().unwrap_or(0.0),
             &record[8], &record[9], record[10].parse::<i64>().unwrap_or(0), &record[11], &now],
        )?;
    }
    Ok(())
}

// ============================================================
// v0.43.0: CLI bulk import/export (catalog-build workflow)
// ============================================================

/// v0.43.0 report for product-import-csv (CLI bulk entry).
pub struct ImportReport {
    pub total_rows: usize,
    pub imported: Vec<String>,
    pub skipped_dupes: Vec<String>,
    pub failed: Vec<String>,
}

fn csv_opt_trim(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// v0.43.0: Bulk product import for new-catalog builds.
/// CSV schema (13 cols; a header row starting with `sku` is skipped):
///   sku,name,category,color,design,season,cost,sale,retail,brand,fabric,qty,purchase_price
/// Duplicate SKUs are SKIPPED and reported; bad rows are reported — never a
/// silent mid-file abort. Each valid row goes through catalog::add_product
/// (the EXACT INSERT the GUI form + product-add CLI run), so the
/// stock_quantity/qty_in_head_office lockstep holds automatically.
pub fn import_products_csv(conn: &Connection, csv_path: &Path) -> Result<ImportReport, Box<dyn std::error::Error>> {
    let file = fs::File::open(csv_path)?;
    let mut rdr = ReaderBuilder::new().has_headers(false).from_reader(file);
    let mut report = ImportReport {
        total_rows: 0,
        imported: Vec::new(),
        skipped_dupes: Vec::new(),
        failed: Vec::new(),
    };

    for (idx, result) in rdr.records().enumerate() {
        let row_no = idx + 1;
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                report.failed.push(format!("row {}: csv parse: {}", row_no, e));
                continue;
            }
        };
        if record.is_empty() || record.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        if record[0].trim().eq_ignore_ascii_case("sku") {
            continue; // header row
        }
        if record.len() < 13 {
            report.failed.push(format!("row {}: {} columns (need 13)", row_no, record.len()));
            continue;
        }
        report.total_rows += 1;
        let sku = record[0].trim().to_string();
        let name = record[1].trim().to_string();
        if sku.is_empty() || name.is_empty() {
            report.failed.push(format!("row {}: sku and name must be non-empty", row_no));
            continue;
        }
        let exists: Option<i64> = conn
            .query_row("SELECT id FROM products WHERE sku = ?1", [&sku], |r| r.get(0))
            .ok();
        if exists.is_some() {
            report.skipped_dupes.push(sku);
            continue;
        }
        let parse_num = |label: &str, raw: &str, def: f64| -> Result<f64, String> {
            let t = raw.trim();
            if t.is_empty() {
                return Ok(def);
            }
            t.parse::<f64>().map_err(|_| format!("{} '{}' is not numeric", label, t))
        };
        let cost = match parse_num("cost", &record[6], 0.0) {
            Ok(v) => v,
            Err(e) => { report.failed.push(format!("row {}: {}", row_no, e)); continue; }
        };
        let sale = match parse_num("sale", &record[7], 0.0) {
            Ok(v) => v,
            Err(e) => { report.failed.push(format!("row {}: {}", row_no, e)); continue; }
        };
        let retail = match parse_num("retail", &record[8], sale) {
            Ok(v) => v,
            Err(e) => { report.failed.push(format!("row {}: {}", row_no, e)); continue; }
        };
        let qty_raw = record[11].trim();
        let qty: i64 = if qty_raw.is_empty() {
            0
        } else {
            match qty_raw.parse::<i64>() {
                Ok(v) => v,
                Err(_) => {
                    report.failed.push(format!("row {}: qty '{}' is not an integer", row_no, qty_raw));
                    continue;
                }
            }
        };
        if qty < 0 {
            report.failed.push(format!("row {}: qty must be >= 0", row_no));
            continue;
        }
        let purchase = match parse_num("purchase_price", &record[12], cost) {
            Ok(v) => v,
            Err(e) => { report.failed.push(format!("row {}: {}", row_no, e)); continue; }
        };

        let product = Product {
            id: None,
            sku: sku.clone(),
            name,
            category: csv_opt_trim(&record[2]),
            color: csv_opt_trim(&record[3]),
            design: csv_opt_trim(&record[4]),
            season: csv_opt_trim(&record[5]),
            cost_price: cost,
            sale_price: sale,
            purchase_price: purchase,
            description: None,
            tags: None,
            stock_quantity: qty,
            status: "active".to_string(),
            images: "[]".to_string(),
            supplier_id: None,
            created_at: None,
            updated_at: None,
            product_code: None,
            brand: csv_opt_trim(&record[9]),
            fabric: csv_opt_trim(&record[10]),
            size_info: None,
            retail_price: Some(retail),
            discount_price: None,
            qty_in_head_office: Some(qty),
            qty_with_agents: Some(0),
            qty_sold: Some(0),
            qty_reserved: Some(0),
            profit_status: None,
        };
        match add_product(conn, &product) {
            Ok(_) => report.imported.push(sku),
            Err(e) => report.failed.push(format!("row {}: insert {}: {}", row_no, sku, e)),
        }
    }
    Ok(report)
}

/// v0.43.0: Bulk product export — SAME 13-col schema as import_products_csv,
/// so a CSV round-trip (export -> edit -> re-import) keeps working. Exports
/// every product regardless of status (archived/sold_out included). Empty
/// optional fields are written as empty cells.
pub fn export_products_csv(conn: &Connection, csv_path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let products = get_all_products(conn)?;
    let mut wtr = WriterBuilder::new().from_path(csv_path)?;
    wtr.write_record(&["sku", "name", "category", "color", "design", "season",
        "cost", "sale", "retail", "brand", "fabric", "qty", "purchase_price"])?;
    for p in &products {
        wtr.write_record(&[
            p.sku.clone(), p.name.clone(),
            p.category.clone().unwrap_or_default(),
            p.color.clone().unwrap_or_default(),
            p.design.clone().unwrap_or_default(),
            p.season.clone().unwrap_or_default(),
            p.cost_price.to_string(),
            p.sale_price.to_string(),
            p.retail_price.map(|v| v.to_string()).unwrap_or_default(),
            p.brand.clone().unwrap_or_default(),
            p.fabric.clone().unwrap_or_default(),
            p.stock_quantity.to_string(),
            p.purchase_price.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(products.len())
}

pub fn process_and_save_image(src_path: &Path, app_images_dir: &Path, format_type: &str) -> Result<String, Box<dyn std::error::Error>> {
    fs::create_dir_all(app_images_dir)?;
    let uuid_str = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0).to_string();
    let file_name = format!("{}_{}.jpg", uuid_str, format_type);
    let dest_path = app_images_dir.join(&file_name);
    let img = ImageReader::open(src_path)?.decode()?;
    let (width, height) = match format_type {
        "instagram" => (1080, 1080),
        "facebook" => (1200, 630),
        "whatsapp" => (800, 800),
        // v0.14.10: Increased thumbnail size from 200x200 to 1200x1200.
        // Previously images were stored at 200x200 (75x compression from a
        // typical 1500x1500 upload) — fine for grid display but pixelated
        // when shared to FB/IG which display at 600-1080px. 1200x1200 is
        // slightly above IG's recommended 1080x1080 post size, so shared
        // images look crisp. File size goes from ~15KB to ~150-300KB —
        // manageable for local-first storage. The catalog grid still
        // downscale via CSS object-contain.
        "thumbnail" => (1200, 1200),
        _ => (1080, 1080),
    };
    // Use `resize` (preserves aspect ratio, fits within bounds) for ALL
    // format types — including thumbnails. Previously thumbnails used
    // `resize_to_fill` which cropped to a 1:1 square on disk, and then the
    // frontend's `object-cover` cropped AGAIN at display time, causing a
    // zoomed-in / partially-cropped look (Issue #3 from user feedback).
    //
    // With `resize`, thumbnails keep their original aspect ratio and fit
    // within the bounding box. The frontend uses `object-contain` so the
    // image is letterboxed (not cropped) when displayed in a non-square
    // container.
    let resized = img.resize(width, height, FilterType::Lanczos3);
    resized.save(&dest_path)?;
    Ok(file_name)
}

pub fn process_and_save_image_bytes(img_bytes: &[u8], app_images_dir: &Path, format_type: &str) -> Result<String, Box<dyn std::error::Error>> {
    fs::create_dir_all(app_images_dir)?;
    let uuid_str = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0).to_string();
    let file_name = format!("{}_{}.jpg", uuid_str, format_type);
    let dest_path = app_images_dir.join(&file_name);
    let img = ImageReader::new(Cursor::new(img_bytes))
        .with_guessed_format()?
        .decode()?;
    let (width, height) = match format_type {
        "instagram" => (1080, 1080),
        "facebook" => (1200, 630),
        "whatsapp" => (800, 800),
        // v0.14.10: Increased thumbnail size from 200x200 to 1200x1200.
        // Previously images were stored at 200x200 (75x compression from a
        // typical 1500x1500 upload) — fine for grid display but pixelated
        // when shared to FB/IG which display at 600-1080px. 1200x1200 is
        // slightly above IG's recommended 1080x1080 post size, so shared
        // images look crisp. File size goes from ~15KB to ~150-300KB —
        // manageable for local-first storage. The catalog grid still
        // downscale via CSS object-contain.
        "thumbnail" => (1200, 1200),
        _ => (1080, 1080),
    };
    // Same as above: use `resize` (preserves aspect ratio) instead of
    // `resize_to_fill` (crops) for thumbnails. See comment in
    // `process_and_save_image_file` above for full context.
    let resized = img.resize(width, height, FilterType::Lanczos3);
    resized.save(&dest_path)?;
    Ok(file_name)
}

