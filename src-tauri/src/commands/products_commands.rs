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
// // Products: CRUD, CSV import/export, image upload/base64/URL/share, drafts
// ============================================================

// ==================== CATALOG ====================

#[tauri::command]
pub async fn get_products(state: State<'_, DbState>) -> Result<Vec<Product>, String> {
    let conn = state.0.lock().await;
    catalog::get_all_products(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_product(state: State<'_, DbState>, product: Product) -> Result<i64, String> {
    let conn = state.0.lock().await;
    catalog::add_product(&conn, &product).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_product(state: State<'_, DbState>, product: Product) -> Result<(), String> {
    let conn = state.0.lock().await;
    catalog::update_product(&conn, &product).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_product(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().await;
    catalog::delete_product(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_products_csv(state: State<'_, DbState>) -> Result<String, String> {
    let conn = state.0.lock().await;
    catalog::export_to_csv(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_products_csv(state: State<'_, DbState>, csv_content: String) -> Result<(), String> {
    let conn = state.0.lock().await;
    catalog::import_from_csv(&conn, &csv_content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upload_product_image(src_path: String, format_type: String) -> Result<String, String> {
    let src = Path::new(&src_path);
    if !src.exists() {
        return Err("Source image file does not exist.".to_string());
    }
    let images_dir = utils::get_images_dir();
    catalog::process_and_save_image(src, &images_dir, &format_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_image_as_base64(filename: String) -> Result<String, String> {
    let images_dir = utils::get_images_dir();
    let path = images_dir.join(&filename);
    if !path.exists() {
        return Err(format!("Image not found: {}", filename));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("Failed to read image: {}", e))?;
    let lower = filename.to_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    };
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", mime, b64))
}

#[tauri::command]
pub async fn save_base64_image(base64_data: String, format_type: String) -> Result<String, String> {
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&base64_data)
        .map_err(|e| format!("Base64 decode error: {}", e))?;
    let images_dir = utils::get_images_dir();
    catalog::process_and_save_image_bytes(&raw, &images_dir, &format_type)
        .map_err(|e| e.to_string())
}

/// v0.24.1: Download an image from a URL and save it as a product image.
/// Used by the Catalog form's "Add from URL" button. Many e-commerce sites
/// hotlink-protect their images and return 403 for non-browser Referer
/// headers, so we set a browser-like User-Agent and Referer.
///
/// Reuses the existing `process_and_save_image_bytes` pipeline (resize,
/// convert, save as JPEG) — no new image processing code.
#[tauri::command]
pub async fn save_image_from_url(url: String, format_type: String) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| format!("HTTP client build failed: {}", e))?;

    // Derive a Referer from the URL's origin — many CDNs require this.
    let referer = reqwest::Url::parse(&url)
        .ok()
        .and_then(|parsed| parsed.origin().ascii_serialization().parse().ok())
        .unwrap_or_else(|| String::from("https://www.google.com/"));

    let response = client.get(&url)
        .header("Referer", &referer)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch image: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Image URL returned HTTP {}", response.status()));
    }

    let bytes = response.bytes()
        .await
        .map_err(|e| format!("Failed to read image bytes: {}", e))?;

    let images_dir = utils::get_images_dir();
    catalog::process_and_save_image_bytes(&bytes, &images_dir, &format_type)
        .map_err(|e| e.to_string())
}

/// v0.14.8: Save a product image to a dedicated temp folder
/// (%LOCALAPPDATA%\A-Collection-Share\) and immediately open Windows
/// Explorer with the file highlighted/selected. The user can then drag
/// the highlighted file directly into Facebook/Instagram/WhatsApp post
/// composers — universally supported, 100% reliable on Windows.
///
/// v0.14.7 history: previously used tauri-plugin-shell open(path) which
/// tried to open the image in Windows Photos app. This was unreliable
/// (Photos app sometimes failed to launch, especially on Windows 10 with
/// custom configurations). The new approach uses explorer.exe /select
/// which ALWAYS works — it opens the folder containing the image and
/// selects it, no app association required.
///
/// Returns the full file path so frontend can display it in the alert.
#[tauri::command]
pub async fn save_image_for_share(
    base64_data: String,
    product_name: String,
) -> Result<String, String> {
    use base64::Engine as _;
    // v0.14.9: Strip data URI prefix if present. The get_image_as_base64
    // command returns "data:image/jpeg;base64,/9j/..." — the prefix must
    // be removed before base64 decoding, otherwise decode fails with
    // "InvalidByte" error. This was THE bug causing v0.14.6/7/8 to show
    // "Image save nahi ho payi" alert — the Rust command was throwing
    // a base64 decode error on every call.
    let clean_base64: &str = if base64_data.starts_with("data:") {
        if let Some(idx) = base64_data.find(',') {
            &base64_data[idx + 1..]
        } else {
            &base64_data
        }
    } else {
        &base64_data
    };
    let raw = base64::engine::general_purpose::STANDARD
        .decode(clean_base64)
        .map_err(|e| format!("Base64 decode error: {}", e))?;

    // v0.14.8: Use a dedicated folder in LocalAppData instead of Downloads.
    // Reasons:
    //   1. Keeps user's Downloads folder clean (no share images cluttering)
    //   2. LocalAppData is always writable on Windows
    //   3. Path is predictable: %LOCALAPPDATA%\A-Collection-Share\
    //   4. User doesn't need to find the file — Explorer opens to it
    let target_dir = dirs::data_local_dir()
        .ok_or_else(|| "Could not find LocalAppData directory".to_string())?
        .join("A-Collection-Share");

    // Create the folder if it doesn't exist
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Could not create share folder: {}", e))?;

    // Sanitize product name for filename: replace spaces/special chars with hyphens
    let safe_name: String = product_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() { c }
            else if c == '-' || c == '_' { c }
            else { '-' }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let safe_name = if safe_name.is_empty() { "product".to_string() } else { safe_name };

    // Build filename with timestamp
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y%m%d-%H%M%S").to_string();
    let filename = format!("A-Collection_{}_{}.jpg", safe_name, timestamp);
    let file_path = target_dir.join(&filename);

    // Decode the image and re-save as JPEG (handles PNG/WebP/etc. inputs)
    let img = image::load_from_memory(&raw)
        .map_err(|e| format!("Image decode error: {}", e))?;
    img.save_with_format(&file_path, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Image save error: {}", e))?;

    // v0.14.8: Open Windows Explorer with the file highlighted.
    // `explorer.exe /select,"C:\path\to\file.jpg"` is the canonical Windows
    // way to open a folder and pre-select a file. Always works, no app
    // association needed (unlike shell.open which tries to launch the
    // default image viewer).
    //
    // On non-Windows platforms this is a no-op (the file is still saved,
    // user can find it manually). The Tauri app is Windows-only per the
    // release workflow, so this is acceptable.
    #[cfg(target_os = "windows")]
    {
        let path_str = file_path.to_string_lossy().to_string();
        let select_arg = format!("/select,{}", path_str);
        std::process::Command::new("explorer.exe")
            .arg(&select_arg)
            .spawn()
            .map_err(|e| format!("Could not open Explorer: {}", e))?;
    }

    // Return the full path as a string so frontend can show it to the user
    Ok(file_path.to_string_lossy().to_string())
}

/// v0.20.0: Save drafts (caption + HD images) to a user-selected folder.
///
/// Ali bhai's requirement:
/// - "Save Draft" button in Share Center
/// - User can pick a custom folder
/// - Full caption (text) + all HD images of that product saved to folder
///
/// Architecture:
/// - Frontend opens folder picker via @tauri-apps/plugin-dialog open({ directory: true })
/// - Frontend calls this command with chosen folder_path + product_name + captions + image_filenames
/// - This command creates subfolder: "<safe_product_name>_<timestamp>"
/// - Saves caption as "caption_<platform>.txt" (one file per platform)
/// - If multiple platforms, also saves combined "all_captions.txt"
/// - Copies HD images from AppData images dir to subfolder (prefixed with index for ordering)
/// - Writes README.txt summary in subfolder
///
/// Returns the path of the created subfolder.
#[tauri::command]
pub async fn save_drafts_to_folder_with_path(
    folder_path: String,
    product_name: String,
    captions: std::collections::HashMap<String, String>,
    image_filenames: Vec<String>,
) -> Result<String, String> {
    // Sanitize product name for folder name
    let safe_name: String = product_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() { c }
            else if c == '-' || c == '_' { c }
            else { '-' }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let safe_name = if safe_name.is_empty() { "product".to_string() } else { safe_name };

    // Timestamp for unique subfolder
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let subfolder_name = format!("{}_{}", safe_name, timestamp);
    let subfolder_path = std::path::Path::new(&folder_path).join(&subfolder_name);

    // Create subfolder
    std::fs::create_dir_all(&subfolder_path)
        .map_err(|e| format!("Failed to create folder: {}", e))?;

    // Save captions as separate .txt files (one per platform)
    for (platform, content) in &captions {
        let filename = format!("caption_{}.txt", platform);
        let filepath = subfolder_path.join(&filename);
        std::fs::write(&filepath, content)
            .map_err(|e| format!("Failed to write {}: {}", filename, e))?;
    }

    // If multiple platforms, also save a combined "all_captions.txt"
    if captions.len() > 1 {
        let mut combined = String::new();
        combined.push_str(&format!("=== {} ===\n\n", product_name));
        for (platform, content) in &captions {
            combined.push_str(&format!("--- {} ---\n{}\n\n", platform.to_uppercase(), content));
        }
        let combined_path = subfolder_path.join("all_captions.txt");
        std::fs::write(&combined_path, combined)
            .map_err(|e| format!("Failed to write all_captions.txt: {}", e))?;
    }

    // Copy HD images from AppData images dir to subfolder
    let images_dir = utils::get_images_dir();
    let mut images_copied = 0;
    let mut image_errors: Vec<String> = Vec::new();

    for (idx, filename) in image_filenames.iter().enumerate() {
        let src_path = images_dir.join(filename);
        if !src_path.exists() {
            image_errors.push(format!("Image not found: {}", filename));
            continue;
        }

        // Prefix with index for ordering, keep original filename
        let dest_filename = format!("{:02}_{}", idx + 1, filename);
        let dest_path = subfolder_path.join(&dest_filename);

        match std::fs::copy(&src_path, &dest_path) {
            Ok(_) => images_copied += 1,
            Err(e) => image_errors.push(format!("Copy {}: {}", filename, e)),
        }
    }

    // Build summary message
    let mut summary = format!(
        "Saved to: {}\nCaptions: {} platforms\nImages: {} copied",
        subfolder_path.display(),
        captions.len(),
        images_copied
    );
    if !image_errors.is_empty() {
        summary.push_str(&format!("\nErrors: {}", image_errors.join("; ")));
    }

    // Write summary as README.txt in subfolder
    let readme_path = subfolder_path.join("README.txt");
    let _ = std::fs::write(&readme_path, &summary);

    Ok(subfolder_path.to_string_lossy().to_string())
}

// ============================================================
// v0.33.0: MANUAL SOLD-OUT MARKING
// ============================================================
//
// Bhai ki need: auto-detection of sold-out items unreliable hai (stock fields
// inconsistent for old data). Manual override button add kiya — bhai khud
// decide karega kaunsa item sold out hai.
//
// mark_product_sold_out: sets profit_status='sold_out' WITHOUT touching
// stock fields. Bhai can still see original stock values for audit.
// reactivate_sold_product (v0.30.0): sets profit_status='in_head_office'
// and restores qty_in_head_office.

/// Mark a product as sold out (manual override).
/// Sets profit_status='sold_out'. Stock fields untouched (kept for audit).
/// Bhai uses this when auto-detection fails (data inconsistency).
#[tauri::command]
pub async fn mark_product_sold_out(
    state: State<'_, DbState>,
    product_id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().await;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE products SET profit_status = 'sold_out', updated_at = ?1 WHERE id = ?2",
        rusqlite::params![&now, product_id],
    ).map_err(|e| format!("Failed to mark as sold out: {}", e))?;
    Ok(())
}
