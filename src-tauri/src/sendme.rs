//! Sendme module - Core file transfer functionality using iroh-blobs
//!
//! This module provides send and receive operations similar to n0-computer/sendme,
//! adapted for use within a Tauri mobile application.

use anyhow::{anyhow, Context, Result};
use iroh::{protocol::Router, Endpoint, RelayMode};
use iroh_blobs::{
    api::downloader::Downloader,
    format::collection::Collection,
    protocol::GetRequest,
    store::fs::FsStore,
    ticket::BlobTicket,
    BlobFormat,
    BlobsProtocol,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tracing::{info, error};
use walkdir::WalkDir;

// ─── Public Types ────────────────────────────────────────────────

/// Transfer progress event sent to the frontend
#[derive(Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub status: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub percent: f32,
}

/// Result of a send operation
#[derive(Clone, Serialize, Deserialize)]
pub struct SendResult {
    pub ticket: String,
    pub file_name: String,
    pub file_size: u64,
}

/// Result of a receive operation
#[derive(Clone, Serialize, Deserialize)]
pub struct ReceiveResult {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
}

/// File info result
#[derive(Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
}

// ─── Internal Types ──────────────────────────────────────────────

/// Active send session state
pub struct SendSession {
    router: Router,
    _temp_dir: tempfile::TempDir,
}

/// Global state for managing active sessions
pub struct SendmeState {
    active_send: Mutex<Option<SendSession>>,
}

impl SendmeState {
    pub fn new() -> Self {
        Self {
            active_send: Mutex::new(None),
        }
    }
}

impl Default for SendmeState {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────

/// Calculate total size of a path (file or directory)
fn calculate_size(path: &Path) -> Result<u64> {
    if path.is_file() {
        Ok(std::fs::metadata(path)?.len())
    } else {
        let mut total = 0u64;
        for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        Ok(total)
    }
}

/// Get the file or directory name from a path
fn get_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Emit a progress event to the frontend.
fn emit_progress(app: &AppHandle, event_name: &str, status: &str, bytes: u64, total: u64, percent: f32) {
    let _ = app.emit(event_name, TransferProgress {
        status: status.to_string(),
        bytes_transferred: bytes,
        total_bytes: total,
        percent,
    });
}


// ─── Public API ──────────────────────────────────────────────────

/// Get basic file info (name and size) efficiently without reading contents
pub async fn get_file_info(path: String) -> Result<FileInfo> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(anyhow!("Path does not exist: {}", path));
    }
    
    let name = get_name(&p);
    let size = calculate_size(&p)?;
    
    Ok(FileInfo { name, size })
}

/// Start sending a file or directory.
/// Creates a Collection with proper filenames that receivers can extract.
pub async fn start_send(
    state: &SendmeState,
    path: String,
    app: AppHandle,
) -> Result<SendResult> {
    const EVENT: &str = "send-progress";
    info!("start_send called with path: {}", path);
    
    let path = PathBuf::from(&path);
    if !path.exists() {
        error!("Path does not exist: {}", path.display());
        return Err(anyhow!("Path does not exist: {}", path.display()));
    }

    let file_name = get_name(&path);
    let file_size = calculate_size(&path)?;
    info!("File: {} Size: {} bytes", file_name, file_size);

    emit_progress(&app, EVENT, "Initializing...", 0, file_size, 0.0);

    // Create a temporary store to import file data
    let temp_dir = tempfile::Builder::new()
        .prefix(".iron-send-import-")
        .tempdir()
        .context("Failed to create temp directory")?;
    let store = FsStore::load(temp_dir.path())
        .await
        .context("Failed to create blob store")?;
    let client = store.blobs();

    emit_progress(&app, EVENT, "Importing files...", 0, file_size, 20.0);

    // Build collection (with duplicate-key detection)
    let mut collection = Collection::default();
    let mut seen_keys = HashSet::new();
    
    if path.is_file() {
        info!("Importing single file: {}", file_name);
        let add_outcome = client.add_path(&path).await.context("Failed to add file")?;
        seen_keys.insert(file_name.clone());
        collection.push(file_name.clone(), add_outcome.hash);
    } else {
        info!("Importing directory: {}", path.display());
        let base_path = path.canonicalize().context("Failed to canonicalize path")?;
        
        for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let file_path = entry.path();
                let relative_path = file_path
                    .strip_prefix(&base_path)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .to_string();
                
                if !seen_keys.insert(relative_path.clone()) {
                    return Err(anyhow!("Duplicate entry in collection: {}", relative_path));
                }
                
                info!("Importing: {} -> {}", file_path.display(), relative_path);
                let add_outcome = client.add_path(file_path).await.context("Failed to add file")?;
                collection.push(relative_path, add_outcome.hash);
            }
        }
    }
    
    info!("Collection has {} entries", collection.len());

    // Use shared helper to create session and generate ticket
    // NOTE: We need to re-create the store/blobs since the helper builds its own.
    // For single send we use the temp_dir we already populated.
    emit_progress(&app, EVENT, "Storing collection...", 0, file_size, 40.0);

    let collection_tag = collection.store(store.as_ref())
        .await
        .context("Failed to store collection")?;
    let collection_hash = collection_tag.hash();

    emit_progress(&app, EVENT, "Starting server...", file_size, file_size, 60.0);

    let blobs = BlobsProtocol::new(&store, None);
    let endpoint = Endpoint::builder()
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;

    let router = Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
    router.endpoint().online().await;

    emit_progress(&app, EVENT, "Generating ticket...", file_size, file_size, 80.0);

    let ticket = BlobTicket::new(router.endpoint().addr(), collection_hash, BlobFormat::HashSeq);
    let ticket_string = ticket.to_string();
    info!("Generated ticket: {}", &ticket_string[..50.min(ticket_string.len())]);

    emit_progress(&app, EVENT, "Ready to send", file_size, file_size, 100.0);

    // Store active session
    let mut active = state.active_send.lock().await;
    *active = Some(SendSession {
        router,
        _temp_dir: temp_dir,
    });

    info!("Send session ready!");
    Ok(SendResult {
        ticket: ticket_string,
        file_name,
        file_size,
    })
}

/// Start sending multiple files by path
pub async fn start_send_multiple(
    state: &SendmeState,
    paths: Vec<String>,
    app: AppHandle,
) -> Result<SendResult> {
    const EVENT: &str = "send-progress";
    info!("start_send_multiple called with {} paths", paths.len());
    
    if paths.is_empty() {
        return Err(anyhow!("No paths provided"));
    }

    // Validate paths and calculate total size
    let mut total_size = 0u64;
    for path_str in &paths {
        let path = PathBuf::from(path_str);
        if path.exists() {
            total_size += calculate_size(&path)?;
        } else {
            return Err(anyhow!("Path does not exist: {}", path.display()));
        }
    }

    let first_file_name = get_name(&PathBuf::from(&paths[0]));
    let display_name = if paths.len() == 1 { first_file_name } else { format!("{} items", paths.len()) };

    emit_progress(&app, EVENT, "Initializing...", 0, total_size, 0.0);

    let temp_dir = tempfile::Builder::new()
        .prefix(".iron-send-")
        .tempdir()
        .context("Failed to create temp directory")?;
    let endpoint = Endpoint::builder()
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create endpoint")?;

    emit_progress(&app, EVENT, "Setting up connection...", 0, total_size, 10.0);

    let store = FsStore::load(temp_dir.path())
        .await
        .context("Failed to load store")?;
    let blobs = BlobsProtocol::new(&store, None);
    let client = store.blobs();

    emit_progress(&app, EVENT, "Importing files...", 0, total_size, 20.0);

    // Build collection from all paths (with duplicate-key detection)
    let mut collection = Collection::default();
    let mut seen_keys = HashSet::new();
    
    for path_str in paths {
        let path = PathBuf::from(&path_str);
        if path.is_file() {
            let file_name = get_name(&path);
            if !seen_keys.insert(file_name.clone()) {
                return Err(anyhow!("Duplicate file name in selection: {}", file_name));
            }
            let add_outcome = client.add_path(&path).await.context("Failed to add file")?;
            collection.push(file_name, add_outcome.hash);
        } else {
            let base_path = path.canonicalize().context("Failed to canonicalize path")?;
            let folder_name = get_name(&path);
            for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    let file_path = entry.path();
                    let relative = file_path
                        .strip_prefix(&base_path)
                        .unwrap_or(file_path)
                        .to_string_lossy()
                        .to_string();
                    let relative_path = format!("{}/{}", folder_name, relative);
                    if !seen_keys.insert(relative_path.clone()) {
                        return Err(anyhow!("Duplicate entry in collection: {}", relative_path));
                    }
                    let add_outcome = client.add_path(file_path).await.context("Failed to add file")?;
                    collection.push(relative_path, add_outcome.hash);
                }
            }
        }
    }

    let collection_tag = collection.store(store.as_ref())
        .await
        .context("Failed to store collection")?;
    let collection_hash = collection_tag.hash();

    emit_progress(&app, EVENT, "Starting server...", total_size, total_size, 60.0);

    let router = Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
    router.endpoint().online().await;

    emit_progress(&app, EVENT, "Generating ticket...", total_size, total_size, 80.0);

    let ticket = BlobTicket::new(router.endpoint().addr(), collection_hash, BlobFormat::HashSeq);
    let ticket_string = ticket.to_string();

    emit_progress(&app, EVENT, "Ready to send", total_size, total_size, 100.0);

    let mut active = state.active_send.lock().await;
    *active = Some(SendSession { router, _temp_dir: temp_dir });

    Ok(SendResult {
        ticket: ticket_string,
        file_name: display_name,
        file_size: total_size,
    })
}

/// Cancel active send session
pub async fn cancel_send(state: &SendmeState) -> Result<()> {
    info!("Cancelling send session...");
    let mut active = state.active_send.lock().await;
    if let Some(session) = active.take() {
        session.router.shutdown().await?;
        info!("Send session cancelled");
    }
    Ok(())
}

/// Receive a file using a ticket
pub async fn receive_file(
    ticket_string: String,
    output_dir: String,
    app: AppHandle,
) -> Result<ReceiveResult> {
    const EVENT: &str = "receive-progress";
    info!("receive_file called with ticket length: {}", ticket_string.len());
    info!("Output dir: {}", output_dir);

    // Parse the ticket
    let ticket: BlobTicket = ticket_string
        .parse()
        .context("Invalid ticket format")?;
    info!("Parsed ticket - hash: {}", ticket.hash());

    let output_path = PathBuf::from(&output_dir);
    if !output_path.exists() {
        info!("Creating output directory: {:?}", output_path);
        std::fs::create_dir_all(&output_path)
            .context("Failed to create output directory")?;
    }

    emit_progress(&app, EVENT, "Connecting to sender...", 0, 0, 0.0);

    // Create temporary directory for receiving
    let temp_dir = tempfile::Builder::new()
        .prefix(".iron-recv-")
        .tempdir()
        .context("Failed to create temp directory")?;

    // Initialize iroh endpoint for client
    let endpoint = Endpoint::builder()
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;

    // Create blob store
    let store = FsStore::load(temp_dir.path())
        .await
        .context("Failed to create blob store")?;
    let blobs = BlobsProtocol::new(&store, None);

    emit_progress(&app, EVENT, "Establishing connection...", 0, 0, 10.0);

    // Spawn the router so we can receive data via the protocol
    let router = Router::builder(endpoint.clone())
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
    router.endpoint().online().await;

    emit_progress(&app, EVENT, "Connecting to peer...", 0, 0, 20.0);

    // Get hash, format, and node address from ticket
    let hash = ticket.hash();
    let format = ticket.format();
    let (node_addr, _, _) = ticket.into_parts();

    // Use the client to download
    let client = store.blobs();

    emit_progress(&app, EVENT, "Downloading...", 0, 0, 30.0);

    // Start the download using Downloader
    let downloader = Downloader::new(&store, router.endpoint());
    let request = match format {
        BlobFormat::Raw => GetRequest::blob(hash),
        BlobFormat::HashSeq => GetRequest::all(hash),
    };
    
    downloader.download(request, [node_addr.id])
        .await
        .context("Download failed")?;
    info!("Download complete!");

    // Export files based on format
    match format {
        BlobFormat::HashSeq => {
            info!("Loading collection manifest...");
            let collection = Collection::load(hash, store.as_ref())
                .await
                .context("Failed to load collection")?;
            info!("Collection has {} files", collection.len());
            
            let mut total_size = 0u64;
            let mut file_names = Vec::new();
            for (name, blob_hash) in collection.iter() {
                info!("Exporting: {} (hash: {})", name, blob_hash);
                let file_path = output_path.join(name);
                
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                
                client
                    .export(*blob_hash, &file_path)
                    .await
                    .with_context(|| format!("Failed to export: {}", name))?;
                
                let size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                total_size += size;
                file_names.push(name.to_string());
            }
            
            let file_name = if file_names.len() == 1 {
                file_names.first().cloned().unwrap_or_else(|| format!("received_{}", &hash.to_string()[..8]))
            } else {
                format!("{} files received", file_names.len())
            };
            
            let final_path = if file_names.len() == 1 {
                output_path.join(&file_name)
            } else {
                output_path.to_path_buf()
            };
            
            emit_progress(&app, EVENT, "Complete", total_size, total_size, 100.0);

            router.shutdown().await?;
            info!("Receive complete!");
            
            Ok(ReceiveResult {
                file_path: final_path.to_string_lossy().to_string(),
                file_name,
                file_size: total_size,
            })
        }
        BlobFormat::Raw => {
            let file_name = format!("received_{}", &hash.to_string()[..8]);
            let final_path = output_path.join(&file_name);
            
            client
                .export(hash, &final_path)
                .await
                .context("Failed to export blob")?;
            
            let file_size = std::fs::metadata(&final_path)
                .map(|m| m.len())
                .unwrap_or(0);

            emit_progress(&app, EVENT, "Complete", file_size, file_size, 100.0);

            router.shutdown().await?;
            info!("Receive complete!");
            
            Ok(ReceiveResult {
                file_path: final_path.to_string_lossy().to_string(),
                file_name,
                file_size,
            })
        }
    }
}
