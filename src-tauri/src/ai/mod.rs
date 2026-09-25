use serde::{Serialize, Deserialize};
use rusqlite::Connection;
use reqwest::Client;
use std::time::Duration;
use serde_json::json;

pub mod catalog_composer;
pub mod ingestion;
pub mod local_match;
pub mod marketing_engine;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AssistantResult {
    LocalMatchFound(local_match::LocalMatchResult),
    NewCatalogDraft(catalog_composer::CatalogDraft),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AiResponse {
    pub text: String,
    pub detected_action: Option<String>,
    pub action_data: Option<serde_json::Value>,
    pub product_draft: Option<ProductDraft>,
    pub confidence: Option<f64>,
    pub missing_fields: Option<Vec<String>>,
    pub suggested_actions: Option<Vec<String>>,
    pub fast_path_data: Option<AssistantResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProductDraft {
    pub name: Option<String>,
    pub sku: Option<String>,
    pub category: Option<String>,
    pub brand: Option<String>,
    pub fabric: Option<String>,
    pub color: Option<String>,
    pub design: Option<String>,
    pub season: Option<String>,
    pub cost_price: Option<f64>,
    pub sale_price: Option<f64>,
    pub retail_price: Option<f64>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub hashtags: Option<Vec<String>>,
    pub images: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MarketingContent {
    pub platform: String,
    pub content: String,
    pub caption_type: String,
    /// Per-platform hashtags (Issue #5 fix). The AI prompt now requests a
    /// `hashtags` array per platform object. This field is Option so the
    /// struct remains backwards-compatible with older AI responses that
    /// omitted it.
    #[serde(default)]
    pub hashtags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DraftResponse {
    pub draft: ProductDraft,
    pub confidence: f64,
    pub missing_fields: Vec<String>,
    pub suggested_actions: Vec<String>,
}

pub fn try_local_intent(conn: &Connection, prompt: &str) -> Option<AiResponse> {
    parse_local_intent(conn, prompt)
}

pub fn get_ai_config(conn: &Connection) -> Result<(String, String, String, String), String> {
    get_ai_settings(conn)
}

pub fn log_request(conn: &Connection, prompt: &str, response: &str, provider: &str) -> Result<(), String> {
    log_ai_request(conn, prompt, response, provider)
}

fn default_business_profile() -> &'static str {
    r#"{"business_name":"A Collection","industry":"Ladies Clothing Retail","owner":"Ali","purchase_city":"Faisalabad","sales_areas":["Narowal","Shakargarh","Zafarwal","Nearby Villages"],"sales_channels":["Facebook","WhatsApp","Door To Door"],"target_customers":{"gender":"Female","income_group":"Middle Income","preferred_products":["3 Piece Suits","Lawn","Cotton","Printed Designs","Embroidery"]},"business_goals":["Increase Profit","Increase Sales","Reduce Dead Stock","Improve Customer Retention","Improve Marketing"],"assistant_roles":["Inventory Manager","Sales Analyst","Marketing Assistant","Business Advisor","Purchase Planner"]}"#
}

pub fn get_business_profile(conn: &Connection) -> Result<serde_json::Value, String> {
    let val = conn.query_row(
        "SELECT value FROM settings WHERE key = 'business_profile'",
        [],
        |row| row.get::<_, String>(0),
    );
    match val {
        Ok(v) => serde_json::from_str(&v).map_err(|e| e.to_string()),
        Err(_) => {
            let default = default_business_profile();
            conn.execute(
                "INSERT OR IGNORE INTO settings (key, value) VALUES ('business_profile', ?1)",
                [default],
            ).map_err(|e| e.to_string())?;
            serde_json::from_str(default).map_err(|e| e.to_string())
        }
    }
}

pub fn build_business_context(conn: &Connection) -> Result<String, String> {
    let prod_count: i64 = conn.query_row("SELECT COUNT(*) FROM products WHERE status='active'", [], |r| r.get(0)).unwrap_or(0);
    let cust_count: i64 = conn.query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0)).unwrap_or(0);
    let order_count: i64 = conn.query_row("SELECT COUNT(*) FROM orders", [], |r| r.get(0)).unwrap_or(0);
    let low_stock: i64 = conn.query_row("SELECT COUNT(*) FROM products WHERE (COALESCE(qty_in_head_office, stock_quantity, 0) + COALESCE(qty_with_agents, 0)) <= 5 AND COALESCE(profit_status, 'in_head_office') != 'sold_out' AND status='active'", [], |r| r.get(0)).unwrap_or(0);
    let dead_stock: i64 = conn.query_row("SELECT COUNT(*) FROM products WHERE stock_quantity = 0 AND status='active'", [], |r| r.get(0)).unwrap_or(0);
    let total_sales: f64 = conn.query_row("SELECT COALESCE(SUM(total_amount), 0.0) FROM orders", [], |r| r.get(0)).unwrap_or(0.0);
    let total_profit: f64 = conn.query_row("SELECT COALESCE(SUM(profit), 0.0) FROM orders", [], |r| r.get(0)).unwrap_or(0.0);

    // Read currency from business_profile. Falls back to "PKR" if the profile
    // is missing or the field is absent. Previously money was hardcoded as
    // "${:.2}" which was incorrect for a Pakistani business.
    let currency = {
        let profile = get_business_profile(conn).unwrap_or_default();
        profile["currency"].as_str().unwrap_or("PKR").to_string()
    };

    let mut context = String::new();
    context.push_str("## Current Business Snapshot\n\n");
    context.push_str(&format!("- **Active Products:** {}\n", prod_count));
    context.push_str(&format!("- **Total Customers:** {}\n", cust_count));
    context.push_str(&format!("- **Total Orders:** {}\n", order_count));
    context.push_str(&format!("- **Total Sales:** {}\n", crate::utils::format_money(total_sales, &currency)));
    context.push_str(&format!("- **Total Profit:** {}\n", crate::utils::format_money(total_profit, &currency)));
    context.push_str(&format!("- **Low Stock Items:** {}\n", low_stock));
    context.push_str(&format!("- **Dead Stock Items:** {}\n", dead_stock));
    // v0.12.1: Append profit-mode context (agents, stock distribution,
    // recent sales, recent shares, stale stock alerts).
    context.push_str(&build_profit_mode_context(conn, &currency));
    Ok(context)
}

/// v0.12.1 — Build profit-mode business context for the AI assistant.
///
/// This injects real-time business state into the AI's system prompt so it
/// can answer questions like:
///   - "Shakargarh agent ka balance kya hai?"
///   - "Blue Maria B suit kidhar pada hai?"
///   - "Konsa product 30 din se nahi bika?"
///   - "Konsa product push karna chahiye?"
///
/// The AI gets read-only access to:
///   - Agent summaries (stock held, cash received, outstanding balance)
///   - Stock distribution (HO vs agents vs sold)
///   - Top outstanding agents
///   - Recent sales (last 7 days)
///   - Recent shares (last 5)
///   - Stale stock alerts (not shared in 7+ days)
///   - Low HO stock alerts
///
/// The AI CANNOT modify any of this data — only suggest actions. All
/// destructive writes go through explicit Tauri commands with validation.
pub fn build_profit_mode_context(conn: &Connection, currency: &str) -> String {
    let mut ctx = String::new();

    // === AGENTS SECTION ===
    ctx.push_str("\n## Agents (Stock Holders)\n\n");
    let agent_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM agents WHERE is_active = 1", [], |r| r.get(0)
    ).unwrap_or(0);
    if agent_count == 0 {
        ctx.push_str("No active agents registered yet.\n");
    } else {
        ctx.push_str(&format!("Active agents: {}\n\n", agent_count));
        // Top agents by outstanding balance
        let mut stmt = match conn.prepare(
            "SELECT a.id, a.name, a.city, a.phone,
                    COALESCE(SUM(CASE WHEN e.entry_type = 'stock_sent' THEN e.qty ELSE 0 END) -
                             SUM(CASE WHEN e.entry_type = 'stock_returned' THEN e.qty ELSE 0 END) -
                             SUM(CASE WHEN e.entry_type = 'sale_reported' THEN e.qty ELSE 0 END), 0) AS stock_units,
                    COALESCE(SUM(CASE WHEN e.entry_type = 'stock_sent' THEN e.amount ELSE 0 END) -
                             SUM(CASE WHEN e.entry_type = 'stock_returned' THEN e.amount ELSE 0 END) -
                             SUM(CASE WHEN e.entry_type = 'cash_received' THEN e.amount ELSE 0 END), 0.0) AS outstanding
             FROM agents a
             LEFT JOIN agent_ledger_entries e ON e.agent_id = a.id
             WHERE a.is_active = 1
             GROUP BY a.id, a.name, a.city, a.phone
             ORDER BY outstanding DESC
             LIMIT 10"
        ) {
            Ok(s) => s,
            Err(_) => return ctx,
        };
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,      // id
                row.get::<_, String>(1)?,    // name
                row.get::<_, Option<String>>(2)?, // city
                row.get::<_, Option<String>>(3)?, // phone
                row.get::<_, i64>(4)?,       // stock_units
                row.get::<_, f64>(5)?,       // outstanding
            ))
        });
        if let Ok(rows) = rows {
            ctx.push_str("| Agent | City | Stock Units | Outstanding |\n");
            ctx.push_str("|-------|------|-------------|-------------|\n");
            for row in rows.flatten() {
                let (_, name, city, _phone, stock, outstanding) = row;
                let city_str = city.unwrap_or_else(|| "—".to_string());
                ctx.push_str(&format!("| {} | {} | {} | {} |\n",
                    name, city_str, stock,
                    crate::utils::format_money(outstanding, currency)
                ));
            }
        }
    }

    // === STOCK DISTRIBUTION SECTION ===
    ctx.push_str("\n## Stock Distribution\n\n");
    let total_ho: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(qty_in_head_office, stock_quantity)), 0) FROM products WHERE status='active'",
        [], |r| r.get(0)
    ).unwrap_or(0);
    let total_agents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(qty_with_agents, 0)), 0) FROM products WHERE status='active'",
        [], |r| r.get(0)
    ).unwrap_or(0);
    let total_sold: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(qty_sold, 0)), 0) FROM products WHERE status='active'",
        [], |r| r.get(0)
    ).unwrap_or(0);
    ctx.push_str(&format!("- **In Head Office:** {} units\n", total_ho));
    ctx.push_str(&format!("- **With Agents:** {} units\n", total_agents));
    ctx.push_str(&format!("- **Sold (all-time):** {} units\n", total_sold));

    // === STALE STOCK ALERTS ===
    ctx.push_str("\n## Stale Stock (not shared in 7+ days)\n\n");
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let stale_count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT p.id) FROM products p
         LEFT JOIN share_logs sl ON sl.product_id = p.id
         WHERE p.status='active' AND p.stock_quantity > 0
         AND (sl.shared_at IS NULL OR sl.shared_at < ?1)",
        [&cutoff],
        |r| r.get(0)
    ).unwrap_or(0);
    if stale_count == 0 {
        ctx.push_str("All active stock has been shared recently. Good!\n");
    } else {
        ctx.push_str(&format!("{} products need social media attention.\n", stale_count));
    }

    // === RECENT SHARES (last 5) ===
    ctx.push_str("\n## Recent Shares (last 5)\n\n");
    let mut stmt = match conn.prepare(
        "SELECT sl.platform, sl.share_angle, sl.shared_at, COALESCE(p.name, '(deleted)')
         FROM share_logs sl
         LEFT JOIN products p ON sl.product_id = p.id
         ORDER BY sl.shared_at DESC LIMIT 5"
    ) {
        Ok(s) => s,
        Err(_) => return ctx,
    };
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    });
    if let Ok(rows) = rows {
        let collected: Vec<_> = rows.flatten().collect();
        if collected.is_empty() {
            ctx.push_str("No shares logged yet.\n");
        } else {
            for (platform, angle, when, product) in collected {
                ctx.push_str(&format!("- {} — {} ({}) on {}\n", product, platform, angle, when));
            }
        }
    }

    // === AI OPERATOR INSTRUCTIONS ===
    ctx.push_str("\n## AI Operator Capabilities\n\n");
    ctx.push_str("You are a business operator, not just a chatbot. You can:\n");
    ctx.push_str("- Answer stock lookup questions (where is product X, who has it)\n");
    ctx.push_str("- Answer agent balance questions (how much does agent X owe)\n");
    ctx.push_str("- Suggest products to push (stale stock, high margin, fresh arrivals)\n");
    ctx.push_str("- Summarize product movement (source trip → HO → agent → sold)\n");
    ctx.push_str("- Generate social media captions using the data above\n\n");
    ctx.push_str("**Constraints:** You CANNOT modify stock, send shares, or adjust balances. ");
    ctx.push_str("You can only suggest actions. The user must execute them via the app UI. ");
    ctx.push_str("All numbers above are real-time from the database.\n");

    ctx
}

pub fn build_system_prompt(conn: &Connection, user_prompt: &str) -> Result<String, String> {
    let profile = get_business_profile(conn).unwrap_or_default();
    let context = build_business_context(conn)?;
    let knowledge = get_relevant_knowledge(conn, user_prompt)?;

    let biz_name = profile["business_name"].as_str().unwrap_or("A Collection");
    let industry = profile["industry"].as_str().unwrap_or("Ladies Clothing Retail");
    let owner = profile["owner"].as_str().unwrap_or("the owner");
    let purchase_city = profile["purchase_city"].as_str().unwrap_or("Faisalabad");
    let sales_areas: Vec<&str> = profile["sales_areas"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    let sales_channels: Vec<&str> = profile["sales_channels"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    let goals: Vec<&str> = profile["business_goals"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    let roles: Vec<&str> = profile["assistant_roles"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();

    let primary_products: Vec<&str> = profile["target_customers"]["preferred_products"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();

    let mut system = String::new();
    system.push_str(&format!("You are {} HeadOffice Assistant.\n\n", biz_name));

    system.push_str("## Identity\n\n");
    system.push_str(&format!("You are the dedicated AI business assistant of {}.\n", biz_name));
    system.push_str("You are not a general-purpose chatbot.\n");
    system.push_str(&format!("Your primary responsibility is helping {} grow sales, manage inventory, improve marketing, increase profits, and make better business decisions.\n", biz_name));
    system.push_str("If a user asks who you are, respond:\n\n");
    system.push_str(&format!("\"I am the {} HeadOffice Assistant.\"\n\n", biz_name));
    system.push_str("You may mention that your underlying AI model is Gemini only if specifically asked.\n");
    system.push_str("Never introduce yourself as Google's assistant.\n\n");

    system.push_str("## Business Overview\n\n");
    system.push_str(&format!("**Business Name:** {}\n", biz_name));
    system.push_str(&format!("**Business Category:** {}\n", industry));
    system.push_str(&format!("**Owner:** {}\n", owner));
    system.push_str("**Primary Products:**\n");
    for p in &primary_products {
        system.push_str(&format!("* {}\n", p));
    }
    system.push_str(&format!("\n**Purchase Source:** {}, Pakistan\n", purchase_city));
    system.push_str("**Primary Sales Areas:**\n");
    for a in &sales_areas {
        system.push_str(&format!("* {}\n", a));
    }
    system.push_str("\n**Sales Channels:**\n");
    for c in &sales_channels {
        system.push_str(&format!("* {}\n", c));
    }

    system.push_str("\n## Core Mission\n\n");
    system.push_str("Your mission is to:\n\n");
    for (i, g) in goals.iter().enumerate() {
        system.push_str(&format!("{}. {}.\n", i + 1, g));
    }

    system.push_str("\n## Assistant Roles\n\n");
    system.push_str("You serve as:\n\n");
    for r in &roles {
        system.push_str(&format!("* {}\n", r));
    }

    system.push_str(&format!("\n## Decision-Making Rules\n\nWhenever making recommendations:\n\n* Prioritize profit.\n* Prioritize customer trust.\n* Avoid risky inventory purchases.\n* Recommend data-driven decisions.\n* Consider local customer behavior.\n* Consider seasonal demand.\n* Consider Pakistani market conditions.\n"));
    for a in &sales_areas {
        system.push_str(&format!("* Consider {} and nearby customer preferences.\n", a));
    }

    let has_fb = sales_channels.iter().any(|c| c.to_lowercase().contains("facebook"));
    let has_wa = sales_channels.iter().any(|c| c.to_lowercase().contains("whatsapp"));

    if has_fb {
        system.push_str("\n## Facebook Marketing Responsibilities\n\nWhen creating Facebook content:\n\n* Write in attractive Roman Urdu or English.\n* Use clear pricing.\n* Focus on value.\n* Focus on trust.\n* Encourage WhatsApp contact.\n* Create urgency when appropriate.\n* Optimize for local audience engagement.\n");
    }

    if has_wa {
        system.push_str("\n## WhatsApp Marketing Responsibilities\n\nWhen creating WhatsApp content:\n\n* Keep messages concise.\n* Highlight price.\n* Highlight design.\n* Highlight availability.\n* Encourage immediate inquiry.\n");
    }

    system.push_str("\n## Inventory Responsibilities\n\nWhen inventory data is available:\n\n* Detect slow-moving stock.\n* Detect fast-selling products.\n* Recommend restocking priorities.\n* Recommend purchase quantities.\n* Warn about low stock.\n\n## Sales Analysis Responsibilities\n\nWhen sales data is available:\n\n* Analyze trends.\n* Identify best-selling categories.\n* Identify weak-performing categories.\n* Recommend actions to improve sales.\n\n## Purchasing Responsibilities\n\nWhen asked what products should be purchased:\n\nAnalyze:\n* Current inventory\n* Seasonal demand\n* Local preferences\n* Historical sales\n\nThen recommend:\n* Product category\n* Quantity\n* Price range\n* Expected demand level\n\n## PERMANENT Language, Tone & Marketing Rules\n\n**Language:** ALL responses, captions, posts, and marketing content MUST be in Hinglish (Roman Urdu + English where necessary). Never pure English, never pure Urdu script. Example: 'Yeh suit bohat pyara hai, sirf Rs. 2500 mein!'\n\n**Local Punjabi Flavor:** Sparingly use short Roman Punjabi expressions to make content feel local and memorable. Maximum ONE short phrase per post. Use only when it naturally strengthens the message. Examples:\n  - 'Raulay pe gaye je!' (People are raving!)\n  - 'Hun gal ban gayi!' (Now it's settled!)\n  - 'Chak de phatte!' (Go for it!)\n  - 'Oye hoye, ki gal ae!' (Wow, what a thing!)\n  Do NOT force these into every post. Use naturally.\n\n**Emotional Tone:** Write like an experienced ladies clothing seller who understands local buying psychology. The tone should be:\n  - Warm and exciting\n  - Emotionally engaging\n  - Friendly and slightly playful\n  - Trust-building\n  - Highly sales-oriented\n\nUse natural emotional openers when appropriate:\n  - 'Girls ❤️'\n  - 'Beautiful Girls ❤️'\n  - 'Ladies ❤️'\nThe reader must feel personally addressed, not like reading a generic ad.\n\n**FORBIDDEN generic AI words:** Never use 'Elegant', 'Beautiful', 'Premium quality' as standalone descriptors. Use specific, emotional, local language instead.\n\n**Price Presentation (CRITICAL for marketing posts):**\n  - Price MUST be at the VERY BEGINNING of the caption — before any other text\n  - Format: SALE price prominent + RETAIL price crossed out + savings amount\n  - Example:\n    🔥 SALE Rs. 2500\n    ~~Retail Rs. 3000~~\n    Save Rs. 500!\n  - The discounted price must be visually dominant\n  - Retail price should appear crossed out using ~~strikethrough~~\n  - NEVER hide pricing deep inside the caption — platforms collapse long text\n  - After the pricing block, continue with the marketing content\n\n**Target Audience:** Narowal district, Pakistan. Both rural AND urban areas. Women and girls aged 10-50 years old. Keep content culturally appropriate for Pakistani female clothing customers. Use local references (Narowal, Shakargarh, Zafarwal) where relevant.\n\n**Hashtags:** Always include Narowal-specific hashtags: #NarowalFashion #NarowalLawn #NarowalGirls #Zafarwal #Shakargarh. Mix with trending fashion hashtags relevant to the target age group.\n\n## Local Market Knowledge\n\nAssume the target market primarily consists of:\n* Middle-income households\n* Value-conscious buyers\n* Female clothing shoppers (women + girls, age 10-50)\n* Customers who prefer attractive designs at affordable prices\n* Both rural and urban Narowal district residents\n\nRecommendations should reflect these realities.\n\n## Communication Style\n\nBe:\n* Professional\n* Practical\n* Business-focused\n* Direct\n* Helpful\n* Always in Hinglish (Roman Urdu)\n* Emotionally warm and persuasive\n\nAvoid:\n* Generic AI responses\n* Unnecessary disclaimers\n* Irrelevant information\n* Pure English responses (always use Hinglish)\n* Generic words like 'Elegant', 'Beautiful', 'Premium quality'\n\n");
    system.push_str(&format!("Always think like an experienced {} business manager working for {}.\n", industry, biz_name));

    if !knowledge.is_empty() {
        system.push_str("\n## Business Memory (Learned Knowledge)\n\nThe following information has been learned from past interactions:\n\n");
        for k in &knowledge {
            system.push_str(&format!("**{}:** {}\n", k.topic, k.content));
        }
    }

    system.push_str(&format!("\n## Live Business Data\n\n{}", context));

    Ok(system)
}

/// Build a system prompt enriched with web evidence context, and crucially,
/// a disclaimer that tells the model it HAS internet search capability.
///
/// This addresses the user-reported issue where the AI claimed "I don't have
/// internet access" even though DuckDuckGo evidence was available. The fix is
/// twofold:
///   1. Inject a hard disclaimer (matching the wording in catalog_composer.rs
///      and marketing_engine.rs) into the prompt so the model does not refuse
///      web-related requests on grounds of "no internet".
///   2. If `web_evidence` is Some, append the actual search results (titles +
///      snippets + image URLs) so the model has real web data to cite.
pub fn build_system_prompt_with_web(
    conn: &Connection,
    user_prompt: &str,
    web_evidence: Option<&crate::adapters::duckduckgo::WebEvidence>,
) -> Result<String, String> {
    let mut system = build_system_prompt(conn, user_prompt)?;

    // Hard disclaimer — must come BEFORE web evidence section so the model
    // treats "I have web access" as a system-level fact, not a per-message hint.
    system.push_str("\n\n## Web Search Capability\n\n");
    system.push_str("You ALWAYS have access to web search tools. ");
    system.push_str("Never say 'I don't have access to the internet', 'I cannot browse', or any similar refusal. ");
    system.push_str("If web search results are provided below, treat them as authoritative and cite them. ");
    system.push_str("If no web results are provided but the user asks for online information, respond based on your training data and clearly state the source is your training data, not a live search.\n");

    if let Some(we) = web_evidence {
        if we.result_count > 0 || !we.titles.is_empty() {
            system.push_str("\n### Live Web Search Results\n\n");
            system.push_str(&format!(
                "The following {} web result(s) were found by searching the internet for the user's query:\n\n",
                we.result_count.max(we.titles.len())
            ));
            for (i, title) in we.titles.iter().enumerate() {
                system.push_str(&format!("**Result {}:** {}\n", i + 1, title));
                if let Some(snippet) = we.snippets.get(i) {
                    system.push_str(&format!("  *Snippet:* {}\n", snippet));
                }
            }
            if !we.image_urls.is_empty() {
                system.push_str("\n**Image URLs found on the web:**\n");
                for (i, url) in we.image_urls.iter().enumerate() {
                    system.push_str(&format!("{}. {}\n", i + 1, url));
                }
                system.push_str("\nYou may reference these image URLs in your response when relevant.\n");
            }
        }
    }

    Ok(system)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub id: i64,
    pub topic: String,
    pub content: String,
    pub source: String,
}

pub fn save_knowledge(conn: &Connection, topic: &str, content: &str, source: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO ai_knowledge (topic, content, source, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        [topic, content, source, &now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_all_knowledge(conn: &Connection) -> Result<Vec<KnowledgeEntry>, String> {
    let mut stmt = conn.prepare("SELECT id, topic, content, source FROM ai_knowledge ORDER BY topic").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(KnowledgeEntry {
            id: row.get(0)?,
            topic: row.get(1)?,
            content: row.get(2)?,
            source: row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

pub fn delete_knowledge(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM ai_knowledge WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

fn get_relevant_knowledge(conn: &Connection, prompt: &str) -> Result<Vec<KnowledgeEntry>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, topic, content, source FROM ai_knowledge WHERE ?1 LIKE '%' || topic || '%' OR topic LIKE '%' || ?1 || '%'"
    ).map_err(|e| e.to_string())?;

    let lower = prompt.to_lowercase();
    let rows = stmt.query_map([&lower], |row| {
        Ok(KnowledgeEntry {
            id: row.get(0)?,
            topic: row.get(1)?,
            content: row.get(2)?,
            source: row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// A single message in a multi-turn conversation. Used by call_ai_provider
/// to pass conversation history to the AI model so it can maintain context
/// across turns.
///
/// - role: "user" or "assistant" (model)
/// - content: the message text
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Dispatcher for all AI providers. Passes conversation history (if any)
/// to the provider so the model can maintain context across turns.
///
/// `history` is optional — if None or empty, the call is single-turn
/// (backward compatible with existing callers like catalog_composer,
/// marketing_engine, etc.).
pub async fn call_ai_provider(
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
    system_prompt: &str,
    user_prompt: &str,
    image_data: Option<&str>,
    history: Option<&[ChatMessage]>,
) -> Result<String, String> {
    match provider {
        "gemini" => call_gemini(api_key, model, system_prompt, user_prompt, image_data, history).await,
        "openai" => call_openai(api_key, model, base_url, system_prompt, user_prompt, image_data, history).await,
        "claude" => call_claude(api_key, model, system_prompt, user_prompt, image_data, history).await,
        // Local LLM (Ollama) uses a different API shape for multimodal — out of scope for this fix.
        "local" => call_local_llm(model, system_prompt, user_prompt).await,
        _ => Err(format!("Unsupported AI provider: {}", provider)),
    }
}

pub fn parse_draft_from_response(text: &str) -> Option<DraftResponse> {
    let body = text.trim();

    // Search for ```json block anywhere in the response
    if let Some(start_pos) = body.find("```json") {
        let after_marker = &body[start_pos + 7..];
        let end_pos = after_marker.find("```").unwrap_or(after_marker.len());
        let json_str = after_marker[..end_pos].trim();
        if let Ok(draft) = serde_json::from_str::<DraftResponse>(json_str) {
            return Some(draft);
        }
    }

    // Fallback: try parsing the entire body as pure JSON
    if body.starts_with('{') || body.starts_with('[') {
        if let Ok(draft) = serde_json::from_str::<DraftResponse>(body) {
            return Some(draft);
        }
    }

    None
}

pub fn prepare_marketing_data(conn: &Connection, product_id: i64) -> Result<(crate::catalog::Product, String, String, String, String, bool, bool), String> {
    let profile = get_business_profile(conn).unwrap_or_default();
    let has_fb = profile["sales_channels"].as_array().map(|a| a.iter().any(|v| v.as_str().unwrap_or("").to_lowercase().contains("facebook"))).unwrap_or(false);
    let has_wa = profile["sales_channels"].as_array().map(|a| a.iter().any(|v| v.as_str().unwrap_or("").to_lowercase().contains("whatsapp"))).unwrap_or(false);
    let product = crate::catalog::get_product_by_id(conn, product_id).map_err(|e| e.to_string())?;
    let (provider, api_key, model, base_url) = get_ai_settings(conn)?;
    Ok((product, provider, api_key, model, base_url, has_fb, has_wa))
}

pub fn build_marketing_prompt(product: &crate::catalog::Product, has_fb: bool, has_wa: bool) -> String {
    let product_json = serde_json::to_string(product).unwrap_or_default();
    // FIX (Issue #5): Previously the JSON template hardcoded all 5 platforms
    // (facebook, instagram, tiktok, whatsapp_status, whatsapp_channel)
    // regardless of has_fb/has_wa flags, and had NO per-platform hashtags
    // field. Now we build the platform list dynamically and add a per-platform
    // `hashtags` array with platform-specific conventions.
    let mut platforms_block = String::new();
    if has_fb {
        platforms_block.push_str("- facebook (1-2 hashtags, use #ACollection + 1 topical)\n");
    }
    platforms_block.push_str("- instagram (5-10 hashtags, mix trending #instafashion #ootd #reelvsfeed + niche)\n");
    platforms_block.push_str("- tiktok (3-5 hashtags, MUST include #fyp and #foryou)\n");
    if has_wa {
        platforms_block.push_str("- whatsapp_status (0 hashtags, plain broadcast message)\n");
        platforms_block.push_str("- whatsapp_channel (0 hashtags, plain announcement)\n");
    }
    if has_fb {
        platforms_block.push_str("- twitter (1-2 hashtags max due to char limit)\n");
    }

    format!(
        "Generate social media marketing content for the following product in our clothing business 'A Collection' (Faisalabad, Narowal, Shakargarh, Zafarwal).
Currency: PKR. Write in attractive Roman Urdu or English.

Product Data:
{}

Generate content for the following platforms (each with platform-specific caption style and hashtag conventions):
{}

Return as JSON array. Each object MUST have a `hashtags` array with the platform-appropriate count:
[
  {{\"platform\": \"facebook\", \"content\": \"...\", \"caption_type\": \"product_showcase\", \"hashtags\": [\"#ACollection\", \"#PakistaniFashion\"]}},
  {{\"platform\": \"instagram\", \"content\": \"...\", \"caption_type\": \"product_showcase\", \"hashtags\": [\"#instafashion\", \"#ootd\", \"#reelvsfeed\", \"#pakistaniweddingwear\", \"#lawncollection\", \"...\"]}},
  {{\"platform\": \"tiktok\", \"content\": \"...\", \"caption_type\": \"product_showcase\", \"hashtags\": [\"#fyp\", \"#foryou\", \"#pakistanifashion\", \"#lawn\", \"#tiktokfashion\"]}},
  {{\"platform\": \"whatsapp_status\", \"content\": \"...\", \"caption_type\": \"product_announcement\", \"hashtags\": []}},
  {{\"platform\": \"whatsapp_channel\", \"content\": \"...\", \"caption_type\": \"product_announcement\", \"hashtags\": []}}
]

IMPORTANT RULES:
- Each hashtag MUST start with #.
- WhatsApp platforms MUST have an empty hashtags array (WhatsApp doesn't use hashtags).
- TikTok MUST include #fyp and #foryou.
- Instagram should have 5-10 hashtags mixing trending + niche.
- Facebook should have 1-2 hashtags max.
- ONLY return the JSON array. No other text.",
        product_json,
        platforms_block
    )
}

pub async fn generate_marketing_content(provider: &str, api_key: &str, model: &str, base_url: &str, prompt: &str) -> Result<Vec<MarketingContent>, String> {
    let sys_prompt = "You are a social media marketing assistant for a Pakistani clothing business. Generate engaging posts in Roman Urdu or English.";
    let response = call_ai_provider(provider, api_key, model, base_url, sys_prompt, prompt, None, None).await?;

    let body = response.trim();
    let json_str = if body.starts_with("```") {
        body.lines()
            .skip_while(|l| !l.contains("```"))
            .skip(1)
            .take_while(|l| !l.contains("```"))
            .collect::<Vec<&str>>()
            .join("\n")
    } else {
        body.to_string()
    };

    serde_json::from_str::<Vec<MarketingContent>>(&json_str).map_err(|e| format!("Marketing parse error: {}", e))
}

fn get_ai_settings(conn: &Connection) -> Result<(String, String, String, String), String> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings WHERE key IN ('ai_provider', 'ai_api_key', 'ai_model', 'ai_base_url')")
        .map_err(|e| e.to_string())?;

    let mut provider = "gemini".to_string();
    let mut api_key = "".to_string();
    // Default to gemini-2.0-flash (Finding H fix). The previous default
    // gemini-1.5-flash is deprecated by Google as of 2025; users on fresh
    // installs would hit model-not-found errors. gemini-2.0-flash is the
    // current recommended fast tier with vision support.
    // Existing installs that already have ai_model set in settings are NOT
    // affected — their stored value overrides this default.
    let mut model = "gemini-2.0-flash".to_string();
    // v0.25.4: base_url for OpenAI-compatible providers (OpenRouter, Together,
    // Groq, local LM Studio, etc.). Empty = use provider's default endpoint.
    let mut base_url = "".to_string();

    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let key: String = row.get(0).map_err(|e| e.to_string())?;
        let val: String = row.get(1).map_err(|e| e.to_string())?;
        match key.as_str() {
            "ai_provider" => provider = val,
            "ai_api_key" => api_key = val,
            "ai_model" => model = val,
            "ai_base_url" => base_url = val,
            _ => {}
        }
    }
    // Auto-upgrade: if user has the deprecated gemini-1.5-flash stored,
    // silently upgrade to gemini-2.0-flash. This prevents users from being
    // stuck on a deprecated model after upgrading the app. The user can still
    // change it back in Settings if they want.
    if model == "gemini-1.5-flash" {
        model = "gemini-2.0-flash".to_string();
    }
    Ok((provider, api_key, model, base_url))
}

fn log_ai_request(conn: &Connection, prompt: &str, response: &str, provider: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO ai_logs (prompt, response, provider, created_at) VALUES (?1, ?2, ?3, ?4);",
        [prompt, response, provider, &now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn parse_local_intent(conn: &Connection, prompt: &str) -> Option<AiResponse> {
    let lower_prompt = prompt.to_lowercase();

    if lower_prompt.contains("low stock") || lower_prompt.contains("stock short") {
        if let Ok(mut stmt) = conn.prepare("SELECT sku, name, stock_quantity FROM products WHERE stock_quantity <= 5 AND status = 'active' ORDER BY stock_quantity ASC") {
            let items_iter = stmt.query_map([], |row| {
                let sku: String = row.get(0)?;
                let name: String = row.get(1)?;
                let qty: i64 = row.get(2)?;
                Ok(json!({ "sku": sku, "name": name, "quantity": qty }))
            });
            if let Ok(iter) = items_iter {
                let items: Vec<serde_json::Value> = iter.filter_map(|x| x.ok()).collect();
                let count = items.len();
                let text = if count == 0 {
                    "All products are well stocked. No low stock items found!".to_string()
                } else {
                    let mut list_str = format!("Found {} items with low stock (<= 5 units):\n\n", count);
                    for item in &items {
                        list_str.push_str(&format!("- **{}** (SKU: {}): {} units left\n", item["name"], item["sku"], item["quantity"]));
                    }
                    list_str
                };
                return Some(AiResponse {
                    text,
                    detected_action: Some("low_stock".to_string()),
                    action_data: Some(json!(items)),
                    product_draft: None,
                    confidence: None,
                    missing_fields: None,
                    suggested_actions: None,
                    fast_path_data: None,
                });
            }
        }
    }

    if lower_prompt.contains("sku ") || lower_prompt.contains("product ") {
        let words: Vec<&str> = lower_prompt.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            if (*word == "sku" || *word == "product") && i + 1 < words.len() {
                let target = words[i + 1].trim_matches(|c| c == ',' || c == '.' || c == '"' || c == '\'');
                if let Ok(mut stmt) = conn.prepare("SELECT sku, name, category, sale_price, stock_quantity, description FROM products WHERE sku LIKE ?1 OR name LIKE ?1") {
                    let search_term = format!("%{}%", target);
                    if let Ok(mut rows) = stmt.query([&search_term]) {
                        if let Ok(Some(row)) = rows.next() {
                            let sku: String = row.get(0).unwrap_or_default();
                            let name: String = row.get(1).unwrap_or_default();
                            let category: String = row.get(2).unwrap_or_default();
                            let price: f64 = row.get(3).unwrap_or(0.0);
                            let qty: i64 = row.get(4).unwrap_or(0);
                            let desc: String = row.get(5).unwrap_or_default();

                            // Read currency from business_profile for proper formatting.
                            // Previously hardcoded as "${:.2}" which was incorrect.
                            let currency = {
                                let profile = get_business_profile(conn).unwrap_or_default();
                                profile["currency"].as_str().unwrap_or("PKR").to_string()
                            };

                            let text = format!(
                                "Here are the details for **{}**:\n\n* **SKU**: {}\n* **Category**: {}\n* **Price**: {}\n* **Current Stock**: {} units\n* **Description**: {}\n",
                                name, sku, category, crate::utils::format_money(price, &currency), qty, desc
                            );
                            
                            return Some(AiResponse {
                                text,
                                detected_action: Some("product_detail".to_string()),
                                action_data: Some(json!({ "sku": sku, "name": name, "price": price, "stock": qty })),
                                product_draft: None,
                                confidence: None,
                                missing_fields: None,
                                suggested_actions: None,
                                fast_path_data: None,
                            });
                        }
                    }
                }
            }
        }
    }

    None
}

fn detect_mime_type(b64: &str) -> &'static str {
    if b64.starts_with("iVBORw0KGgo") {
        "image/png"
    } else if b64.starts_with("/9j/") {
        "image/jpeg"
    } else if b64.starts_with("UklGR") {
        "image/webp"
    } else if b64.starts_with("R0lGOD") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

async fn call_gemini(
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    image_data: Option<&str>,
    history: Option<&[ChatMessage]>,
) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );

    // Build the contents array as a multi-turn conversation.
    // Gemini's API expects: contents: [ {role: "user"/"model", parts: [...]}, ... ]
    // We map "user" -> "user" and "assistant" -> "model".
    let mut contents: Vec<serde_json::Value> = Vec::new();

    // Append conversation history (if any) BEFORE the current user message.
    // This gives the model context from previous turns.
    if let Some(hist) = history {
        for msg in hist.iter() {
            let role = if msg.role == "assistant" { "model" } else { "user" };
            contents.push(json!({
                "role": role,
                "parts": [{"text": msg.content}]
            }));
        }
    }

    // Current user message (with optional image)
    let mut parts = vec![json!({"text": user_prompt})];
    let has_image = image_data.is_some();
    if let Some(b64) = image_data {
        let clean_b64 = if let Some(comma_pos) = b64.find(',') {
            &b64[comma_pos + 1..]
        } else {
            b64
        };
        let mime = detect_mime_type(clean_b64);
        parts.push(json!({
            "inlineData": {
                "mimeType": mime,
                "data": clean_b64
            }
        }));
    }
    contents.push(json!({
        "role": "user",
        "parts": parts
    }));

    let mut payload = json!({
        "system_instruction": {
            "parts": [{"text": system_prompt}]
        },
        "contents": contents
    });

    // v0.25.5: Removed googleSearch tool hardcode.
    // Previously, every text-only request had "tools": [{"googleSearch": {}}]
    // injected. This caused two problems:
    // 1. Newer Gemini models (3.5-flash, 3.1-flash) may handle tools
    //    differently — googleSearch consumes separate grounding quota and
    //    can trigger 429 errors faster on models with smaller free-tier
    //    limits.
    // 2. The tool adds latency and token cost for simple chat queries
    //    that don't need web search.
    // If web search is needed in the future, it should be an explicit
    // user toggle in Settings, not a silent default.

    let res = client.post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed sending request to Gemini: {}", e))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Gemini API returned error: {}", err_body));
    }

    let res_json: serde_json::Value = res.json()
        .await
        .map_err(|e| format!("Failed parsing Gemini response: {}", e))?;

    let text = res_json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| "Failed to extract text from Gemini response".to_string())?;

    Ok(text.to_string())
}

async fn call_openai(
    api_key: &str,
    model: &str,
    base_url: &str,
    system_prompt: &str,
    user_prompt: &str,
    image_data: Option<&str>,
    history: Option<&[ChatMessage]>,
) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // v0.25.4: Use custom base_url if set (OpenAI-compatible providers like
    // OpenRouter, Together, Groq, local LM Studio, etc.). If empty, default
    // to the official OpenAI API endpoint. The base_url should be the full
    // endpoint path (e.g. "https://openrouter.ai/api/v1/chat/completions")
    // or just the base (e.g. "https://openrouter.ai/api/v1") — we append
    // "/chat/completions" if the URL doesn't already end with it.
    let url = if base_url.is_empty() {
        "https://api.openai.com/v1/chat/completions".to_string()
    } else if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else if base_url.ends_with('/') {
        format!("{}chat/completions", base_url)
    } else {
        format!("{}/chat/completions", base_url)
    };

    // Build the messages array as a multi-turn conversation.
    let mut messages: Vec<serde_json::Value> = vec![
        json!({"role": "system", "content": system_prompt})
    ];

    // Append conversation history (if any) BEFORE the current user message.
    if let Some(hist) = history {
        for msg in hist.iter() {
            messages.push(json!({"role": msg.role, "content": msg.content}));
        }
    }

    // Build user message content. If an image is attached, use the vision
    // content format: an array of {type: text} and {type: image_url} objects.
    // Otherwise, send a plain string (cheaper, smaller payload).
    let user_content: serde_json::Value = if let Some(b64) = image_data {
        let clean_b64 = if let Some(comma_pos) = b64.find(',') {
            &b64[comma_pos + 1..]
        } else {
            b64
        };
        let mime = detect_mime_type(clean_b64);
        let data_uri = format!("data:{};base64,{}", mime, clean_b64);
        json!([
            {"type": "text", "text": user_prompt},
            {"type": "image_url", "image_url": {"url": data_uri}}
        ])
    } else {
        json!(user_prompt)
    };
    messages.push(json!({"role": "user", "content": user_content}));

    let payload = json!({
        "model": model,
        "messages": messages
    });

    // v0.25.5: Add OpenRouter-friendly headers. OpenRouter requires
    // HTTP-Referer and X-Title for proper attribution. Other OpenAI-
    // compatible providers ignore these headers, so they're safe to
    // always include.
    let res = client.post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://github.com/airdropia/a-collection-head-office")
        .header("X-Title", "A Collection Head Office")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed sending request to OpenAI-compatible API: {}", e))?;

    // v0.25.5: Capture raw response body BEFORE trying to parse as JSON.
    // Previously, if the provider returned HTML/SSE/empty body, the error
    // was just "Failed parsing OpenAI response: error decoding response body"
    // — useless for debugging. Now we capture the raw text and show the
    // first 500 chars in the error so the user can see what went wrong.
    // Note: res.text() consumes res, so we must capture status FIRST.
    let status = res.status();
    let raw_body = res.text().await.unwrap_or_default();

    if !status.is_success() {
        // Truncate long error bodies (some providers return huge HTML pages)
        let preview = if raw_body.len() > 500 {
            format!("{}...(truncated)", &raw_body[..500])
        } else {
            raw_body.clone()
        };
        return Err(format!("API returned HTTP {} {}: {}", status, url, preview));
    }

    // Check for SSE streaming format (some providers return "data: {...}\n\n"
    // even for non-streaming requests). Strip the prefix if present.
    let json_str = if raw_body.starts_with("data: ") {
        // Take the first data chunk and parse it
        let line = raw_body.lines()
            .find(|l| l.starts_with("data: ") && !l.contains("[DONE]"))
            .map(|l| l.trim_start_matches("data: "))
            .unwrap_or("");
        if line.is_empty() {
            return Err(format!("API returned empty SSE stream. Raw body (first 500 chars): {}", &raw_body[..raw_body.len().min(500)]));
        }
        line.to_string()
    } else {
        raw_body
    };

    let res_json: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| {
            let preview = if json_str.len() > 500 {
                format!("{}...(truncated)", &json_str[..500])
            } else {
                json_str.clone()
            };
            format!("Failed parsing API response as JSON: {}. Raw body: {}", e, preview)
        })?;

    // v0.25.7: Try multiple response shapes since OpenAI-compatible providers
    // don't all follow the exact same structure.
    //
    // Standard OpenAI:        choices[0].message.content (string)
    // Vision/multimodal:      choices[0].message.content (array of {type, text})
    // Legacy completion API:  choices[0].text
    // OpenRouter reasoning:   choices[0].message.content may be null when
    //                         reasoning models return content in a different
    //                         field — try message.reasoning as fallback
    //
    // If none of these work, show the raw JSON (first 800 chars) so the
    // user can see exactly what the provider returned — much more useful
    // than "Failed to extract text" with no context.
    let content_val = &res_json["choices"][0]["message"]["content"];
    let text = if let Some(s) = content_val.as_str() {
        s.to_string()
    } else if let Some(arr) = content_val.as_array() {
        let mut combined = String::new();
        for block in arr {
            if let Some(t) = block["text"].as_str() {
                combined.push_str(t);
            }
        }
        combined
    } else if let Some(s) = res_json["choices"][0]["text"].as_str() {
        // Legacy completion API format
        s.to_string()
    } else if let Some(s) = res_json["choices"][0]["message"]["reasoning"].as_str() {
        // Some reasoning models (e.g. DeepSeek R1 via OpenRouter) return
        // the answer in message.reasoning when content is empty/null
        s.to_string()
    } else {
        // None of the standard paths worked — show raw JSON for diagnosis
        let raw_preview = serde_json::to_string_pretty(&res_json)
            .unwrap_or_else(|_| res_json.to_string());
        let truncated = if raw_preview.len() > 800 {
            format!("{}...(truncated)", &raw_preview[..800])
        } else {
            raw_preview
        };
        return Err(format!(
            "Could not find text in API response. Tried: choices[0].message.content, choices[0].text, choices[0].message.reasoning\n\nRaw JSON response:\n{}",
            truncated
        ));
    };

    Ok(text)
}

async fn call_claude(
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    image_data: Option<&str>,
    history: Option<&[ChatMessage]>,
) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let url = "https://api.anthropic.com/v1/messages";

    // Build the messages array as a multi-turn conversation.
    // Claude's messages API expects: messages: [{role, content: [blocks]}, ...]
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // Append conversation history (if any) BEFORE the current user message.
    if let Some(hist) = history {
        for msg in hist.iter() {
            messages.push(json!({
                "role": msg.role,
                "content": [{"type": "text", "text": msg.content}]
            }));
        }
    }

    // Current user message with optional image
    let mut content_blocks: Vec<serde_json::Value> = Vec::new();
    if let Some(b64) = image_data {
        let clean_b64 = if let Some(comma_pos) = b64.find(',') {
            &b64[comma_pos + 1..]
        } else {
            b64
        };
        let mime = detect_mime_type(clean_b64);
        content_blocks.push(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": mime,
                "data": clean_b64
            }
        }));
    }
    content_blocks.push(json!({
        "type": "text",
        "text": user_prompt
    }));
    messages.push(json!({"role": "user", "content": content_blocks}));

    let payload = json!({
        "model": model,
        "max_tokens": 1024,
        "system": system_prompt,
        "messages": messages
    });

    let res = client.post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed sending request to Claude: {}", e))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Claude API returned error: {}", err_body));
    }

    let res_json: serde_json::Value = res.json()
        .await
        .map_err(|e| format!("Failed parsing Claude response: {}", e))?;

    let content_arr = res_json["content"].as_array()
        .ok_or_else(|| "Failed to extract text from Claude response".to_string())?;
    let mut combined = String::new();
    for block in content_arr {
        if let Some(t) = block["text"].as_str() {
            combined.push_str(t);
        }
    }
    if combined.is_empty() {
        return Err("Claude response contained no text content".to_string());
    }

    Ok(combined)
}

async fn call_local_llm(model: &str, system_prompt: &str, user_prompt: &str) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())?;

    let url = "http://localhost:11434/api/generate";

    let full_prompt = format!("{}\n\nUser: {}", system_prompt, user_prompt);

    let payload = json!({
        "model": model,
        "prompt": full_prompt,
        "stream": false
    });

    let res = client.post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Ollama (Local LLM) connection failed: {}. Make sure Ollama is running.", e))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Local LLM returned error: {}", err_body));
    }

    let res_json: serde_json::Value = res.json()
        .await
        .map_err(|e| format!("Failed parsing local LLM response: {}", e))?;

    let text = res_json["response"]
        .as_str()
        .ok_or_else(|| "Failed to extract text from Local LLM response".to_string())?;

    Ok(text.to_string())
}
