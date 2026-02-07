//! Sendme module - Core file transfer functionality using iroh-blobs
//!
//! This module provides send and receive operations similar to n0-computer/sendme,
//! adapted for use within a Tauri mobile application.

use anyhow::{anyhow, Context, Result};
use iroh::{protocol::Router, Endpoint, RelayMode, SecretKey};
use iroh_blobs::{
    api::downloader::Downloader,
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
/// Accepts either a file path OR raw bytes (for Android content:// URIs)
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
        status: "Importing file...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 20.0,
    });

    // Get the client for operations
    let client = store.blobs();

    // Import the file using the client API
    info!("Reading file data...");
    let hash = if path.is_file() {
        // Read file and add to store using client
        let data = tokio::fs::read(&path).await.context("Failed to read file")?;
        info!("Read {} bytes from file", data.len());
        let add_outcome = client.add_bytes(data).await.context("Failed to add bytes")?;
        info!("Added bytes to store, hash: {}", add_outcome.hash);
        add_outcome.hash
    } else {
        // For directories, read all files and create a simple blob
        let mut all_data = Vec::new();
        for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let file_data = tokio::fs::read(entry.path()).await?;
                all_data.extend(file_data);
            }
        }
        info!("Read {} bytes from directory", all_data.len());
        let add_outcome = client.add_bytes(all_data).await.context("Failed to add bytes")?;
        info!("Added bytes to store, hash: {}", add_outcome.hash);
        add_outcome.hash
    };

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

    // Generate ticket with the endpoint address
    let addr = router.endpoint().addr();
    info!("Endpoint address: {:?}", addr);
    let ticket = BlobTicket::new(addr, hash, BlobFormat::Raw);
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

    // Add bytes directly to store
    info!("Adding {} bytes to store...", data.len());
    let add_outcome = client.add_bytes(data).await.context("Failed to add bytes")?;
    let hash = add_outcome.hash;
    info!("Added bytes to store, hash: {}", hash);

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

    // Generate ticket with the endpoint address
    let addr = router.endpoint().addr();
    info!("Endpoint address: {:?}", addr);
    let ticket = BlobTicket::new(addr, hash, BlobFormat::Raw);
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

    // Get hash and node address from ticket
    let hash = ticket.hash();
    let (node_addr, _, _) = ticket.into_parts();
    info!("Attempting to download from node: {:?}", node_addr);

    // Use the client to download
    let client = store.blobs();

    // Download the blob
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Downloading...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 30.0,
    });

    // Start the download using Downloader
    info!("Starting download...");
    let downloader = Downloader::new(&store, router.endpoint());
    let _download = downloader.download(hash, [node_addr.id]).await?;
    info!("Download started, waiting for completion...");
    
    // Emit progress during download
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Transferring data...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 50.0,
    });

    // Download is complete at this point
    info!("Download complete!");

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Saving file...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 80.0,
    });

    // Generate output filename based on hash
    let file_name = format!("received_{}", &hash.to_string()[..8]);
    let final_path = output_path.join(&file_name);
    info!("Exporting to: {:?}", final_path);

    // Export the blob to a file
    info!("Starting export...");
    client
        .export(hash, &final_path)
        .await
        .context("Failed to export blob")?;
    info!("Export complete!");

    let file_size = std::fs::metadata(&final_path)
        .map(|m| m.len())
        .unwrap_or(0);
    info!("Final file size: {} bytes", file_size);

    // Emit complete
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Complete".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 100.0,
    });

    // Cleanup - shutdown router first
    info!("Shutting down router...");
    router.shutdown().await?;
    info!("Receive complete!");

    Ok(ReceiveResult {
        file_path: final_path.to_string_lossy().to_string(),
        file_name,
        file_size,
    })
}
