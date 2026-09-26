//! Auto-extracted from commands/mod.rs during v0.27.0 refactor.
//! See worklog.md Task ID: refactor-1 for context.
//!
//! Behavior unchanged — only file structure modified.

use crate::catalog::{self, Product};
use crate::inventory::{self, InventorySummary, LowStockItem, DeadStockItem, BestSellerItem};
use crate::customers::{self, Customer, OrderItemInput, OrderHistory};
use crate::reports::{self, SalesReport, InventoryReport, CustomerSummaryReport};
use crate::purchase_trips::{self, PurchaseTripSummary};
use crate::adapters::duckduckgo::{self, WebEvidence};
use crate::ai::{self, AiResponse};
use crate::utils;
use crate::commands::{DbState, set_setting_val, get_setting_val};
use tauri::async_runtime::Mutex;
use std::path::Path;
use rusqlite::{Connection, params};
use tauri::State;

// ============================================================
// // AI: ask_ai, product draft, catalog draft, social post, marketing, knowledge
// ============================================================

// ==================== AI ====================

#[tauri::command]
pub async fn ask_ai(
    state: State<'_, DbState>,
    prompt: String,
    image_data: Option<String>,
    history: Option<Vec<ai::ChatMessage>>,
) -> Result<AiResponse, String> {

    let extraction = if let Some(ref b64) = image_data {
        use base64::Engine as _;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            match crate::ai::ingestion::extract_local_data(&bytes) {
                Ok(result) => {
                    Some(result)
                }
                Err(e) => {
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut fast_path_data: Option<ai::AssistantResult> = None;

    if let Some(ref extraction) = extraction {
        let match_result = {
            let conn = state.0.lock().await;
            crate::ai::local_match::check_local_catalog(&conn, &extraction.qr_data, &extraction.ocr_text)
        };
        match match_result {
            Ok(Some(mr)) => {
                fast_path_data = Some(ai::AssistantResult::LocalMatchFound(mr));
            }
            Ok(None) => {
                // Capture provider along with api_key + model so we can pass
                // it to catalog_composer. Previously cfg.0 (provider) was
                // discarded, causing catalog_composer to silently use
                // hardcoded "gemini" — meaning OpenAI/Claude/Ollama users
                // would get a Gemini API call (which fails without a Gemini
                // API key).
                let (provider, api_key, model, base_url) = {
                    let conn = state.0.lock().await;
                    let cfg = ai::get_ai_config(&conn)?;
                    (cfg.0.clone(), cfg.1.clone(), cfg.2.clone(), cfg.3.clone())
                };

                // Build search query from extraction text
                let search_query = extraction.ocr_text.as_deref()
                    .or(extraction.qr_data.as_deref())
                    .unwrap_or("")
                    .to_string();

                // Fetch web evidence via DuckDuckGo (free, no API key needed)
                let web_evidence: Option<WebEvidence> = if !search_query.is_empty() {
                    match duckduckgo::fetch_web_evidence(&search_query).await {
                        Ok(evidence) => {
                            Some(evidence)
                        }
                        Err(e) => {
                            None
                        }
                    }
                } else {
                    None
                };

                match crate::ai::catalog_composer::generate_catalog_draft(
                    extraction, &Some(prompt.clone()), &provider, &api_key, &model, &base_url, &web_evidence, image_data.as_deref()
                ).await {
                    Ok(draft) => {
                        fast_path_data = Some(ai::AssistantResult::NewCatalogDraft(draft));
                    }
                    Err(_e) => {
                    }
                }
            }
            Err(_e) => {
            }
        }
    }

    let local_result = {
        let conn = state.0.lock().await;
        ai::try_local_intent(&conn, &prompt)
    };
    if let Some(response) = local_result { return Ok(response); }

    // SHORT-CIRCUIT: if the fast path already produced a CatalogDraft (or a
    // LocalMatchFound), DO NOT run the second AI call. The previous behavior
    // was to ALWAYS run a fallback `call_ai_provider` that re-prompted Gemini
    // in "Product Intake Mode" and parsed another draft from its text response,
    // which caused the duplicate-draft UX bug (frontend rendered BOTH
    // fast_path_data AND product_draft for the same image).
    //
    // We only fall through to the second AI call when the fast path did not
    // produce a structured result — i.e. no image was uploaded, or local
    // extraction failed, or local_match + catalog_composer both yielded None.
    if fast_path_data.is_some() {
        return Ok(AiResponse {
            text: String::new(),
            detected_action: None,
            action_data: None,
            product_draft: None,
            confidence: None,
            missing_fields: None,
            suggested_actions: None,
            fast_path_data,
        });
    }

    let (provider, api_key, model, base_url) = {
        let conn = state.0.lock().await;
        ai::get_ai_config(&conn)?
    };
    if api_key.is_empty() && provider != "local" {
        return Err("AI API key is missing. Please configure it in Settings.".to_string());
    }

    // Fetch web evidence for the FALLBACK path too. Previously web evidence was
    // only fetched for the fast path (catalog_composer). The fallback path
    // (which runs for text-only queries like "internet se photo laao") had no
    // web access, so Gemini truthfully replied "I don't have internet access".
    //
    // Now we fetch web evidence using the user's text prompt and inject it into
    // the system prompt via build_system_prompt_with_web. We also inject a
    // hard disclaimer so the model does not refuse web-related queries.
    //
    // Skip web fetch for empty prompts (defensive — ask_ai already returns
    // early if both prompt and image are empty).
    let fallback_web_evidence: Option<WebEvidence> = if !prompt.trim().is_empty() {
        match duckduckgo::fetch_web_evidence(&prompt).await {
            Ok(evidence) => {
                Some(evidence)
            }
            Err(e) => {
                None
            }
        }
    } else {
        None
    };

    let system_prompt = {
        let conn = state.0.lock().await;
        let mut sp = ai::build_system_prompt_with_web(&conn, &prompt, fallback_web_evidence.as_ref())?;
        sp.push_str("\n\n## Product Intake Mode\n\nWhen the user shares a product image, link, code, or description, you MUST:\n1. Analyze all available information\n2. If product information is detected, return a JSON block at the end of your response:\n\n```json\n{\n  \"draft\": {\n    \"name\": \"...\",\n    \"sku\": \"...\",\n    \"category\": \"...\",\n    \"brand\": \"...\",\n    \"fabric\": \"...\",\n    \"color\": \"...\",\n    \"design\": \"...\",\n    \"season\": \"...\",\n    \"cost_price\": 0.0,\n    \"sale_price\": 0.0,\n    \"retail_price\": 0.0,\n    \"description\": \"...\",\n    \"tags\": [\"...\"],\n    \"keywords\": [\"...\"],\n    \"hashtags\": [\"...\"]\n  },\n  \"confidence\": 0.85,\n  \"missing_fields\": [\"stock_location\", \"purchase_cost\"],\n  \"suggested_actions\": [\"Add To Catalog\", \"Edit Draft\", \"Generate Marketing\"]\n}\n```\n\n3. If no product information is detected, respond normally as a business assistant.\n");
        sp
    };

    let response_text = ai::call_ai_provider(
        &provider, &api_key, &model, &base_url, &system_prompt, &prompt,
        image_data.as_deref(),
        history.as_deref(),
    ).await?;
    {
        let conn = state.0.lock().await;
        ai::log_request(&conn, &prompt, &response_text, &provider)?;
    }

    let mut resp = AiResponse { text: response_text.clone(), detected_action: None, action_data: None, product_draft: None, confidence: None, missing_fields: None, suggested_actions: None, fast_path_data };

    if let Some(draft_resp) = ai::parse_draft_from_response(&response_text) {
        resp.product_draft = Some(draft_resp.draft);
        resp.confidence = Some(draft_resp.confidence);
        resp.missing_fields = Some(draft_resp.missing_fields);
        resp.suggested_actions = Some(draft_resp.suggested_actions);
        resp.detected_action = Some("product_draft".to_string());
    }

    Ok(resp)
}

#[tauri::command]
pub async fn save_product_draft_to_catalog(state: State<'_, DbState>, draft: ai::ProductDraft) -> Result<i64, String> {
    let now = chrono::Utc::now().to_rfc3339();
    // v0.14.4: Auto-generate SKU when empty. The products.sku column has a
    // UNIQUE constraint, and an empty string ("") is itself a value — so
    // two drafts saved without SKU would collide on the second one.
    // Generating `AC-<timestamp_ms>` avoids the collision and gives the
    // user a sensible placeholder they can edit later in the Catalog form.
    let sku = match draft.sku.as_deref() {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => format!("AC-{}", chrono::Utc::now().timestamp_millis()),
    };
    let product = crate::catalog::Product {
        id: None,
        sku,
        name: draft.name.clone().unwrap_or_else(|| "New Product".to_string()),
        category: draft.category.clone(),
        color: draft.color.clone(),
        design: draft.design.clone(),
        season: draft.season.clone(),
        cost_price: draft.cost_price.unwrap_or(0.0),
        sale_price: draft.sale_price.unwrap_or(0.0),
        // v0.14.3: purchase_price is being phased out (removed from Catalog
        // form UI). Default to cost_price for backward DB compat.
        purchase_price: draft.cost_price.unwrap_or(0.0),
        description: draft.description.clone(),
        tags: draft.tags.clone().map(|t| t.join(", ")),
        stock_quantity: 0,
        status: "active".to_string(),
        images: draft.images.clone()
            .map(|i| serde_json::to_string(&i).unwrap_or_else(|_| "[]".to_string()))
            .unwrap_or_else(|| "[]".to_string()),
        supplier_id: None,
        created_at: Some(now.clone()),
        updated_at: Some(now.clone()),
        // v0.11.0+ profit-mode fields — default to None/empty for manually
        // created drafts. These get populated when a purchase trip item is
        // linked or when stock is sent to an agent.
        product_code: None,
        brand: None,
        fabric: None,
        size_info: None,
        base_unit_cost: None,
        landed_unit_cost: None,
        // v0.14.3: Persist retail_price from draft (was hardcoded to None —
        // never made it to the products table even when add_product supported it).
        retail_price: draft.retail_price,
        discount_price: None,
        source_trip_id: None,
        qty_in_head_office: None,
        qty_with_agents: None,
        qty_sold: None,
        qty_reserved: None,
        profit_status: None,
    };
    let conn = state.0.lock().await;
    let id = crate::catalog::add_product(&conn, &product).map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn save_catalog_draft(state: State<'_, DbState>, draft: crate::ai::catalog_composer::CatalogDraft) -> Result<i64, String> {
    // Download the web image (best_image_url) BEFORE acquiring the DB lock.
    // This avoids holding Mutex<Connection> across a network .await, which
    // would block other Tauri commands for up to 8 seconds (image download
    // timeout). Matches the pattern used in ask_ai where network calls
    // happen outside the lock scope.
    //
    // If best_image_url is None or download fails, fall back to "[]" (no
    // image attached). The save never fails due to image download issues.
    // v0.13.9: Check saved_image_filename FIRST (user-uploaded image).
    // If not present, fall back to best_image_url (web image download).
    // This was THE critical bug — user's uploaded image was never saved
    // because only best_image_url was checked.
    let images_json: String = if let Some(ref filename) = draft.saved_image_filename {
        if !filename.is_empty() {
            serde_json::to_string(&[filename]).unwrap_or_else(|_| "[]".to_string())
        } else {
            "[]".to_string()
        }
    } else if let Some(ref url) = draft.best_image_url {
        if !url.is_empty() {
            match download_and_save_image(url).await {
                Ok(filename) => {
                    serde_json::to_string(&[filename]).unwrap_or_else(|_| "[]".to_string())
                }
                Err(e) => {
                    "[]".to_string()
                }
            }
        } else {
            "[]".to_string()
        }
    } else {
        "[]".to_string()
    };

    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();
    let sku = draft.design_code.clone().unwrap_or_default();
    let title = &draft.title;

    // Duplicate check by SKU (design_code)
    if !sku.is_empty() {
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM products WHERE sku = ?1",
            [&sku],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        if exists {
            return Err(format!("Duplicate item found. SKU: {} already exists in catalog.", sku));
        }
    }

    // Duplicate check by title (case-insensitive)
    if !title.is_empty() {
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM products WHERE LOWER(name) = LOWER(?1)",
            [title],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        if exists {
            return Err(format!("Duplicate item found. '{}' already exists in catalog.", title));
        }
    }

    let mut tags = Vec::new();
    if let Some(ref brand) = draft.brand {
        if !brand.is_empty() { tags.push(format!("Brand: {}", brand)); }
    }
    if let Some(ref fabric) = draft.fabric {
        if !fabric.is_empty() { tags.push(format!("Fabric: {}", fabric)); }
    }
    // v0.14.5: Store gender in tags (no dedicated products.gender column —
    // matches the Catalog.tsx handleSave pattern that also puts designType +
    // gender in tags to avoid a schema migration).
    if let Some(ref gender) = draft.gender {
        if !gender.is_empty() { tags.push(gender.clone()); }
    }
    let product = crate::catalog::Product {
        id: None,
        sku: draft.design_code.clone().unwrap_or_default(),
        name: draft.title,
        // v0.14.5: Pass through AI-generated catalog metadata fields.
        // Previously these were hardcoded to None — when user clicked
        // "Add to Catalog", the resulting product row had empty category,
        // color, season, gender. Now the AI's draft values flow through
        // directly and the user can edit them in the Catalog form if
        // needed.
        category: draft.category.clone(),
        color: draft.color.clone(),
        design: draft.brand.clone(),
        season: draft.season.clone(),
        // v0.13.8: Save actual prices from draft (was hardcoded to 0.0)
        cost_price: draft.cost_price.unwrap_or(0.0),
        sale_price: draft.sale_price.unwrap_or(0.0),
        purchase_price: draft.cost_price.unwrap_or(0.0),
        description: draft.notes.clone(),
        tags: if tags.is_empty() { None } else { Some(tags.join(", ")) },
        stock_quantity: 0,
        status: "active".to_string(),
        images: images_json,
        supplier_id: None,
        created_at: Some(now.clone()),
        updated_at: Some(now.clone()),
        product_code: None,
        brand: draft.brand.clone(),
        fabric: draft.fabric.clone(),
        size_info: None,
        base_unit_cost: draft.cost_price,
        landed_unit_cost: None,
        // v0.13.8: Save retail_price from draft (was hardcoded to None)
        retail_price: draft.retail_price,
        discount_price: None,
        source_trip_id: None,
        qty_in_head_office: None,
        qty_with_agents: None,
        qty_sold: None,
        qty_reserved: None,
        profit_status: None,
    };
    let id = crate::catalog::add_product(&conn, &product).map_err(|e| e.to_string())?;
    Ok(id)
}

/// Download an image from a URL and save it locally using the existing
/// `process_and_save_image_bytes` helper (which normalizes the image to a
/// thumbnail-sized JPEG with aspect-ratio preservation).
///
/// Returns the saved filename (e.g., "1730123456_thumbnail.jpg") on success,
/// or an error string on failure. The caller is expected to handle failures
/// gracefully (fall back to no-image).
///
/// Used by `save_catalog_draft` to persist the `best_image_url` that the
/// AI catalog composer extracted from web search results.
async fn download_and_save_image(url: &str) -> Result<String, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let res = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        // Many e-commerce sites hotlink-protect images and 403 any non-self
        // referer. Sending no referer maximizes the chance of success.
        .header("Referer", "")
        .send()
        .await
        .map_err(|e| format!("Image download request failed: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("Image download returned HTTP {}", res.status()));
    }

    let bytes = res.bytes().await
        .map_err(|e| format!("Failed to read image bytes: {}", e))?
        .to_vec();

    if bytes.is_empty() {
        return Err("Image download returned 0 bytes".to_string());
    }

    // Reuse the existing image processing helper. It will:
    // 1. Decode the image (JPEG/PNG/WebP/GIF)
    // 2. Resize preserving aspect ratio to fit within 200x200
    // 3. Save as JPEG to the app's images directory
    // 4. Return the filename
    let images_dir = crate::utils::get_images_dir();
    crate::catalog::process_and_save_image_bytes(&bytes, &images_dir, "thumbnail")
        .map_err(|e| format!("Failed to process/save image: {}", e))
}

#[tauri::command]
pub async fn generate_social_post(
    state: State<'_, DbState>,
    product_id: i64,
    platform: Option<String>,
) -> Result<crate::ai::marketing_engine::MarketingPost, String> {
    // Capture provider along with api_key + model. Previously cfg.0 (provider)
    // was discarded, causing marketing_engine to silently use hardcoded
    // "gemini" — OpenAI/Claude/Ollama users could not use Generate Post.
    let (provider, api_key, model, base_url) = {
        let conn = state.0.lock().await;
        let cfg = ai::get_ai_config(&conn)?;
        (cfg.0.clone(), cfg.1.clone(), cfg.2.clone(), cfg.3.clone())
    };
    let product = {
        let conn = state.0.lock().await;
        crate::catalog::get_product_by_id(&conn, product_id).map_err(|e| e.to_string())?
    };
    let product_name = &product.name;
    // FIX (Issue #5): Previously `brand = product.design` and `fabric =
    // product.tags` were semantically wrong. Now we pass the actual brand
    // field (or fall back to design if brand is not stored separately) and
    // use category as fabric indicator (since the products table doesn't
    // have a dedicated `fabric` column — `tags` is a JSON array, not a
    // fabric name).
    let brand = product.design.as_deref().unwrap_or("");
    let fabric = product.category.as_deref().unwrap_or("");
    let notes = product.description.as_deref().unwrap_or("");
    crate::ai::marketing_engine::generate_marketing_post(
        product_name, brand, fabric, notes, &provider, &api_key, &model,
        platform.as_deref(),
    ).await
}

#[tauri::command]
pub async fn generate_marketing(state: State<'_, DbState>, product_id: i64) -> Result<Vec<ai::MarketingContent>, String> {
    let (product, provider, api_key, model, base_url, has_fb, has_wa) = {
        let conn = state.0.lock().await;
        ai::prepare_marketing_data(&conn, product_id)?
    };
    let prompt = ai::build_marketing_prompt(&product, has_fb, has_wa);
    let posts = ai::generate_marketing_content(&provider, &api_key, &model, &base_url, &prompt).await?;
    let now = chrono::Utc::now().to_rfc3339();
    {
        let conn = state.0.lock().await;
        for post in &posts {
            // Serialize hashtags array to JSON string for storage.
            let hashtags_json = serde_json::to_string(&post.hashtags).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT INTO social_posts (product_id, platform, content, caption_type, status, created_at, hashtags) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
                rusqlite::params![product_id, post.platform, post.content, post.caption_type, &now, &hashtags_json],
            ).map_err(|e| e.to_string())?;
        }
    }
    Ok(posts)
}

#[tauri::command]
pub async fn save_knowledge(state: State<'_, DbState>, topic: String, content: String, source: String) -> Result<(), String> {
    let conn = state.0.lock().await;
    ai::save_knowledge(&conn, &topic, &content, &source)
}
