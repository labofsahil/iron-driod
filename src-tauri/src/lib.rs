//! Iron - Sendme mobile client
//!
//! A Tauri mobile application for peer-to-peer file transfers using iroh-blobs.

mod sendme;

use sendme::{ReceiveResult, SendResult, SendmeState, FileInfo};
use std::sync::Arc;
use tauri::State;

/// Initialize tracing for debugging
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("debug,iroh=info,iroh_blobs=info"));
    fmt().with_env_filter(filter).init();
}

/// Tauri command: Start sending a file or directory (by path)
#[tauri::command]
async fn start_send(
    path: String,
    state: State<'_, Arc<SendmeState>>,
    app: tauri::AppHandle,
) -> Result<SendResult, String> {
    tracing::info!("start_send command called with path: {}", path);
    sendme::start_send(&state, path, app)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command: Start sending from raw bytes (for Android content:// URIs)
#[tauri::command]
async fn start_send_bytes(
    file_name: String,
    data: Vec<u8>,
    state: State<'_, Arc<SendmeState>>,
    app: tauri::AppHandle,
) -> Result<SendResult, String> {
    tracing::info!("start_send_bytes command called - file: {}, size: {} bytes", file_name, data.len());
    sendme::start_send_bytes(&state, file_name, data, app)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command: Cancel active send session
#[tauri::command]
async fn cancel_send(state: State<'_, Arc<SendmeState>>) -> Result<(), String> {
    sendme::cancel_send(&state)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command: Receive a file using a ticket
#[tauri::command]
async fn receive_file(
    ticket: String,
    output_dir: String,
    app: tauri::AppHandle,
) -> Result<ReceiveResult, String> {
    tracing::info!("receive_file command called");
    sendme::receive_file(ticket, output_dir, app)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command: Get file info quickly
#[tauri::command]
async fn get_file_info(path: String) -> Result<FileInfo, String> {
    sendme::get_file_info(path)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri command: Get downloads directory (platform-specific)
#[tauri::command]
fn get_downloads_dir() -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        Ok("/storage/emulated/0/Download".to_string())
    }
    #[cfg(target_os = "ios")]
    {
        // iOS documents directory
        if let Some(home) = dirs::home_dir() {
            Ok(home.join("Documents").to_string_lossy().to_string())
        } else {
            Err("Could not determine home directory".to_string())
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if let Some(download_dir) = dirs::download_dir() {
            Ok(download_dir.to_string_lossy().to_string())
        } else if let Some(home) = dirs::home_dir() {
            Ok(home.join("Downloads").to_string_lossy().to_string())
        } else {
            Err("Could not determine downloads directory".to_string())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing
    init_tracing();

    // Create shared state
    let sendme_state = Arc::new(SendmeState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(sendme_state)
        .invoke_handler(tauri::generate_handler![
            start_send,
            start_send_bytes,
            cancel_send,
            receive_file,
            get_downloads_dir,
            get_file_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

