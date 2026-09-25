use serde::{Serialize, Deserialize};
use rusqlite::{Connection, params};

#[derive(Debug, Serialize, Deserialize)]
pub struct InventorySummary {
    pub total_products: i64,
    pub total_stock: i64,
    pub total_cost_value: f64,
    pub total_retail_value: f64,
    pub potential_profit: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LowStockItem {
    pub id: i64,
    pub sku: String,
    pub name: String,
    pub stock_quantity: i64,
    pub category: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeadStockItem {
    pub id: i64,
    pub sku: String,
    pub name: String,
    pub stock_quantity: i64,
    pub category: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BestSellerItem {
    pub product_id: i64,
    pub sku: String,
    pub name: String,
    pub quantity_sold: i64,
    pub total_revenue: f64,
    pub total_profit: f64,
}

pub fn get_inventory_summary(conn: &Connection) -> Result<InventorySummary, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT 
            COUNT(*), 
            SUM(stock_quantity), 
            SUM(stock_quantity * cost_price), 
            SUM(stock_quantity * sale_price) 
         FROM products WHERE status = 'active'"
    )?;
    
    let summary = stmt.query_row([], |row| {
        let total_products: i64 = row.get(0)?;
        let total_stock: i64 = row.get(1).unwrap_or(0);
        let total_cost_value: f64 = row.get(2).unwrap_or(0.0);
        let total_retail_value: f64 = row.get(3).unwrap_or(0.0);
        let potential_profit = total_retail_value - total_cost_value;
        
        Ok(InventorySummary {
            total_products,
            total_stock,
            total_cost_value,
            total_retail_value,
            potential_profit,
        })
    })?;

    Ok(summary)
}

pub fn get_low_stock_items(conn: &Connection, threshold: i64) -> Result<Vec<LowStockItem>, rusqlite::Error> {
    // v0.35.0: aligned to the v0.27+ split columns (qty_in_head_office +
    // qty_with_agents) — the legacy stock_quantity-only rule undercounted
    // stock that had moved to agents. Fallbacks keep pre-v0.27 rows working.
    // Excludes sold-out products. Threshold semantics unchanged.
    let mut stmt = conn.prepare(
        "SELECT id, sku, name, stock_quantity, category 
         FROM products 
         WHERE (COALESCE(qty_in_head_office, stock_quantity, 0) + COALESCE(qty_with_agents, 0)) <= ?1 
           AND COALESCE(profit_status, 'in_head_office') != 'sold_out' 
           AND status = 'active'
         ORDER BY stock_quantity ASC"
    )?;

    let iter = stmt.query_map(params![threshold], |row| {
        Ok(LowStockItem {
            id: row.get(0)?,
            sku: row.get(1)?,
            name: row.get(2)?,
            stock_quantity: row.get(3)?,
            category: row.get(4)?,
        })
    })?;

    let mut items = Vec::new();
    for item in iter {
        items.push(item?);
    }
    Ok(items)
}

pub fn get_dead_stock_items(conn: &Connection, days_limit: i64) -> Result<Vec<DeadStockItem>, rusqlite::Error> {
    // Dead stock is defined as items created more than `days_limit` ago 
    // AND have had 0 sales in the last `days_limit` days.
    let cutoff_date = (chrono::Utc::now() - chrono::Duration::days(days_limit)).to_rfc3339();
    
    let mut stmt = conn.prepare(
        "SELECT p.id, p.sku, p.name, p.stock_quantity, p.category, p.created_at 
         FROM products p
         WHERE p.stock_quantity > 0 
           AND p.status = 'active'
           AND p.created_at < ?1
           AND p.id NOT IN (
               SELECT oi.product_id 
               FROM order_items oi
               JOIN orders o ON oi.order_id = o.id
               WHERE o.order_date >= ?1
           )
         ORDER BY p.stock_quantity DESC"
    )?;

    let iter = stmt.query_map([&cutoff_date], |row| {
        Ok(DeadStockItem {
            id: row.get(0)?,
            sku: row.get(1)?,
            name: row.get(2)?,
            stock_quantity: row.get(3)?,
            category: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;

    let mut items = Vec::new();
    for item in iter {
        items.push(item?);
    }
    Ok(items)
}

pub fn get_best_sellers(conn: &Connection, limit: i64) -> Result<Vec<BestSellerItem>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT 
            oi.product_id, 
            p.sku, 
            p.name, 
            SUM(oi.quantity) as qty_sold, 
            SUM(oi.quantity * oi.sale_price) as total_rev,
            SUM(oi.quantity * (oi.sale_price - oi.cost_price)) as total_profit
         FROM order_items oi
         JOIN products p ON oi.product_id = p.id
         GROUP BY oi.product_id
         ORDER BY qty_sold DESC
         LIMIT ?1"
    )?;

    let iter = stmt.query_map(params![limit], |row| {
        Ok(BestSellerItem {
            product_id: row.get(0)?,
            sku: row.get(1)?,
            name: row.get(2)?,
            quantity_sold: row.get(3)?,
            total_revenue: row.get(4)?,
            total_profit: row.get(5)?,
        })
    })?;

    let mut items = Vec::new();
    for item in iter {
        items.push(item?);
    }
    Ok(items)
}

pub fn adjust_stock(conn: &Connection, product_id: i64, adjustment: i64) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE products 
         SET stock_quantity = MAX(0, stock_quantity + ?1), updated_at = ?2
         WHERE id = ?3",
        params![adjustment, now, product_id],
    )?;
    Ok(())
}
