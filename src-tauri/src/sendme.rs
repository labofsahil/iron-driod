//! Sendme module - Core file transfer functionality using iroh-blobs
//!
//! This module provides send and receive operations similar to n0-computer/sendme,
//! adapted for use within a Tauri mobile application.

use anyhow::{anyhow, Context, Result};
use iroh::{protocol::Router, Endpoint, RelayMode, SecretKey};
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
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tracing::{info, error};
use walkdir::WalkDir;

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

/// Start sending a file or directory
/// Creates a Collection with proper filenames that receivers can extract
pub async fn start_send(
    state: &SendmeState,
    path: String,
    app: AppHandle,
) -> Result<SendResult> {
    info!("start_send called with path: {}", path);
    
    let path = PathBuf::from(&path);
    
    if !path.exists() {
        error!("Path does not exist: {}", path.display());
        return Err(anyhow!("Path does not exist: {}", path.display()));
    }

    let file_name = get_name(&path);
    let file_size = calculate_size(&path)?;
    info!("File: {} Size: {} bytes", file_name, file_size);

    // Emit initial progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Initializing...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 0.0,
    });

    // Create temporary directory for blob storage
    let temp_dir = tempfile::Builder::new()
        .prefix(".iron-send-")
        .tempdir()
        .context("Failed to create temp directory")?;
    info!("Created temp dir: {:?}", temp_dir.path());

    // Initialize iroh endpoint
    info!("Creating iroh endpoint...");
    let secret_key = SecretKey::generate(&mut rand::rng());
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;
    info!("Endpoint created successfully");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Setting up connection...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 10.0,
    });

    // Create blob store using FsStore + BlobsProtocol
    info!("Creating blob store...");
    let store = FsStore::load(temp_dir.path())
        .await
        .context("Failed to create blob store")?;
    let blobs = BlobsProtocol::new(&store, None);
    info!("Blob store created");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Importing files...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 20.0,
    });

    // Get the client for operations
    let client = store.blobs();

    // Create a Collection with proper filenames
    let mut collection = Collection::default();
    
    if path.is_file() {
        // Single file: add to collection with its name
        info!("Importing single file: {}", file_name);
        let data = tokio::fs::read(&path).await.context("Failed to read file")?;
        info!("Read {} bytes from file", data.len());
        let add_outcome = client.add_bytes(data).await.context("Failed to add bytes")?;
        info!("Added bytes to store, hash: {}", add_outcome.hash);
        collection.push(file_name.clone(), add_outcome.hash);
    } else {
        // Directory: recursively add all files with relative paths
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
                
                info!("Importing: {} -> {}", file_path.display(), relative_path);
                let file_data = tokio::fs::read(file_path).await?;
                let add_outcome = client.add_bytes(file_data).await.context("Failed to add bytes")?;
                info!("Added file, hash: {}", add_outcome.hash);
                collection.push(relative_path, add_outcome.hash);
            }
        }
    }
    
    info!("Collection has {} entries", collection.len());

    // Store the collection to get its root hash
    info!("Storing collection...");
    let collection_tag = collection.store(store.as_ref())
        .await
        .context("Failed to store collection")?;
    let collection_hash = collection_tag.hash();
    info!("Collection stored, hash: {}", collection_hash);

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Starting server...".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 60.0,
    });

    // Build router with blobs protocol - this starts serving the blobs
    info!("Spawning router...");
    let router = Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
    info!("Router spawned");

    // Wait for the endpoint to be online (connected to relay)
    info!("Waiting for relay connection...");
    router.endpoint().online().await;
    info!("Connected to relay!");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Generating ticket...".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 80.0,
    });

    // Generate ticket with the endpoint address - use HashSeq format for collections
    let addr = router.endpoint().addr();
    info!("Endpoint address: {:?}", addr);
    let ticket = BlobTicket::new(addr, collection_hash, BlobFormat::HashSeq);
    let ticket_string = ticket.to_string();
    info!("Generated ticket: {}", &ticket_string[..50.min(ticket_string.len())]);

    // Emit complete
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Ready to send".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 100.0,
    });

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

/// Start sending from raw bytes (for Android content:// URIs)
pub async fn start_send_bytes(
    state: &SendmeState,
    file_name: String,
    data: Vec<u8>,
    app: AppHandle,
) -> Result<SendResult> {
    let file_size = data.len() as u64;
    info!("start_send_bytes called - file: {}, size: {} bytes", file_name, file_size);

    // Emit initial progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Initializing...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 0.0,
    });

    // Create temporary directory for blob storage
    let temp_dir = tempfile::Builder::new()
        .prefix(".iron-send-")
        .tempdir()
        .context("Failed to create temp directory")?;
    info!("Created temp dir: {:?}", temp_dir.path());

    // Initialize iroh endpoint
    info!("Creating iroh endpoint...");
    let secret_key = SecretKey::generate(&mut rand::rng());
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;
    info!("Endpoint created successfully");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Setting up connection...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 10.0,
    });

    // Create blob store using FsStore + BlobsProtocol
    info!("Creating blob store...");
    let store = FsStore::load(temp_dir.path())
        .await
        .context("Failed to create blob store")?;
    let blobs = BlobsProtocol::new(&store, None);
    info!("Blob store created");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Importing file...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 20.0,
    });

    // Get the client for operations
    let client = store.blobs();

    // Add bytes directly to store and create Collection with filename
    info!("Adding {} bytes to store...", data.len());
    let add_outcome = client.add_bytes(data).await.context("Failed to add bytes")?;
    info!("Added bytes to store, hash: {}", add_outcome.hash);
    
    // Create a Collection with the filename
    let mut collection = Collection::default();
    collection.push(file_name.clone(), add_outcome.hash);
    
    // Store the collection to get its root hash
    info!("Storing collection...");
    let collection_tag = collection.store(store.as_ref())
        .await
        .context("Failed to store collection")?;
    let collection_hash = collection_tag.hash();
    info!("Collection stored, hash: {}", collection_hash);

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Starting server...".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 60.0,
    });

    // Build router with blobs protocol - this starts serving the blobs
    info!("Spawning router...");
    let router = Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
    info!("Router spawned");

    // Wait for the endpoint to be online (connected to relay)
    info!("Waiting for relay connection...");
    router.endpoint().online().await;
    info!("Connected to relay!");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Generating ticket...".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 80.0,
    });

    // Generate ticket with the endpoint address - use HashSeq format for collections
    let addr = router.endpoint().addr();
    info!("Endpoint address: {:?}", addr);
    let ticket = BlobTicket::new(addr, collection_hash, BlobFormat::HashSeq);
    let ticket_string = ticket.to_string();
    info!("Generated ticket: {}", &ticket_string[..50.min(ticket_string.len())]);

    // Emit complete
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Ready to send".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 100.0,
    });

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

/// Cancel active send session
pub async fn cancel_send(state: &SendmeState) -> Result<()> {
    info!("Cancelling send session...");
    let mut active = state.active_send.lock().await;
    if let Some(session) = active.take() {
        // Shutdown the router gracefully
        session.router.shutdown().await?;
        info!("Send session cancelled");
        // temp_dir is automatically cleaned up when dropped
    }
    Ok(())
}

/// Receive a file using a ticket
pub async fn receive_file(
    ticket_string: String,
    output_dir: String,
    app: AppHandle,
) -> Result<ReceiveResult> {
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

    // Emit initial progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Connecting to sender...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 0.0,
    });

    // Create temporary directory for receiving
    let temp_dir = tempfile::Builder::new()
        .prefix(".iron-recv-")
        .tempdir()
        .context("Failed to create temp directory")?;
    info!("Created temp dir: {:?}", temp_dir.path());

    // Initialize iroh endpoint for client
    info!("Creating iroh endpoint...");
    let secret_key = SecretKey::generate(&mut rand::rng());
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;
    info!("Endpoint created");

    // Create blob store
    info!("Creating blob store...");
    let store = FsStore::load(temp_dir.path())
        .await
        .context("Failed to create blob store")?;
    let blobs = BlobsProtocol::new(&store, None);
    info!("Blob store created");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Establishing connection...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 10.0,
    });

    // Spawn the router so we can receive data via the protocol
    info!("Spawning router...");
    let router = Router::builder(endpoint.clone())
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
    info!("Router spawned");

    // Wait for endpoint to be online
    info!("Waiting for relay connection...");
    router.endpoint().online().await;
    info!("Connected to relay!");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Connecting to peer...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 20.0,
    });

    // Get hash, format, and node address from ticket
    let hash = ticket.hash();
    let format = ticket.format();
    let (node_addr, _, _) = ticket.into_parts();
    info!("Attempting to download from node: {:?}, format: {:?}", node_addr, format);

    // Use the client to download
    let client = store.blobs();

    // Download the blob
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Downloading...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 30.0,
    });

    // Start the download using Downloader with proper request based on format
    info!("Starting download...");
    let downloader = Downloader::new(&store, router.endpoint());
    
    // Create the appropriate request based on blob format
    let request = match format {
        BlobFormat::Raw => GetRequest::blob(hash),
        BlobFormat::HashSeq => GetRequest::all(hash),
    };
    
    // Start the download and await its completion
    let download_progress = downloader.download(request, [node_addr.id]);
    download_progress.await.context("Download failed")?;
    info!("Download complete!");

    // Export files based on format
    match format {
        BlobFormat::HashSeq => {
            // For HashSeq (collection), load the collection manifest to get filenames
            info!("Loading collection manifest...");
            // Use store as reference - FsStore implements Deref to Store which implements SimpleStore
            let collection = Collection::load(hash, store.as_ref())
                .await
                .context("Failed to load collection")?;
            
            info!("Collection has {} files", collection.len());
            
            // Export each file in the collection
            let mut total_size = 0u64;
            let mut file_names = Vec::new();
            for (name, blob_hash) in collection.iter() {
                info!("Exporting: {} (hash: {})", name, blob_hash);
                let file_path = output_path.join(name);
                
                // Create parent directories if needed
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
            
            info!("Export complete! Total size: {} bytes", total_size);
            
            // Get the first filename for the result
            let file_name = file_names.first().cloned().unwrap_or_else(|| format!("received_{}", &hash.to_string()[..8]));
            let final_path = if file_names.len() == 1 {
                output_path.join(&file_name)
            } else {
                output_path.to_path_buf()
            };
            
            // Emit complete
            let _ = app.emit("transfer-progress", TransferProgress {
                status: "Complete".to_string(),
                bytes_transferred: total_size,
                total_bytes: total_size,
                percent: 100.0,
            });
            
            // Cleanup - shutdown router
            info!("Shutting down router...");
            router.shutdown().await?;
            info!("Receive complete!");
            
            Ok(ReceiveResult {
                file_path: final_path.to_string_lossy().to_string(),
                file_name,
                file_size: total_size,
            })
        }
        BlobFormat::Raw => {
            // For raw blobs, use hash-based name
            let file_name = format!("received_{}", &hash.to_string()[..8]);
            let final_path = output_path.join(&file_name);
            info!("Exporting to: {:?}", final_path);
            
            client
                .export(hash, &final_path)
                .await
                .context("Failed to export blob")?;
            
            let file_size = std::fs::metadata(&final_path)
                .map(|m| m.len())
                .unwrap_or(0);
            info!("Export complete! File size: {} bytes", file_size);
            
            // Emit complete
            let _ = app.emit("transfer-progress", TransferProgress {
                status: "Complete".to_string(),
                bytes_transferred: file_size,
                total_bytes: file_size,
                percent: 100.0,
            });
            
            // Cleanup - shutdown router
            info!("Shutting down router...");
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
