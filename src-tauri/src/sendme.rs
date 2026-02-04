//! Sendme module - Core file transfer functionality using iroh-blobs
//!
//! This module provides send and receive operations similar to n0-computer/sendme,
//! adapted for use within a Tauri mobile application.

use anyhow::{anyhow, Context, Result};
use iroh::{protocol::Router, Endpoint, RelayMode, SecretKey};
use iroh_blobs::{
    net_protocol::Blobs,
    ticket::BlobTicket,
    BlobFormat,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
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
pub async fn start_send(
    state: &SendmeState,
    path: String,
    app: AppHandle,
) -> Result<SendResult> {
    let path = PathBuf::from(&path);
    
    if !path.exists() {
        return Err(anyhow!("Path does not exist: {}", path.display()));
    }

    let file_name = get_name(&path);
    let file_size = calculate_size(&path)?;

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

    // Initialize iroh endpoint
    let secret_key = SecretKey::generate(rand::rngs::OsRng);
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Setting up connection...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 10.0,
    });

    // Create persistent blob store using the builder pattern
    let blobs = Blobs::persistent(temp_dir.path())
        .await
        .context("Failed to create blob store")?
        .build(&endpoint);

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Importing file...".to_string(),
        bytes_transferred: 0,
        total_bytes: file_size,
        percent: 20.0,
    });

    // Get the client for operations
    let client = blobs.client();

    // Import the file using the client API
    let hash = if path.is_file() {
        // Read file and add to store using client
        let data = tokio::fs::read(&path).await.context("Failed to read file")?;
        let add_outcome = client.add_bytes(data).await.context("Failed to add bytes")?;
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
        let add_outcome = client.add_bytes(all_data).await.context("Failed to add bytes")?;
        add_outcome.hash
    };

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Generating ticket...".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 80.0,
    });

    // Build router with blobs protocol
    let router = Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, blobs.clone())
        .spawn()
        .await
        .context("Failed to start protocol router")?;

    // Generate ticket
    let node_addr = router.endpoint().node_addr().await?;
    let ticket = BlobTicket::new(node_addr, hash, BlobFormat::Raw)?;
    let ticket_string = ticket.to_string();

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

    Ok(SendResult {
        ticket: ticket_string,
        file_name,
        file_size,
    })
}

/// Cancel active send session
pub async fn cancel_send(state: &SendmeState) -> Result<()> {
    let mut active = state.active_send.lock().await;
    if let Some(session) = active.take() {
        // Shutdown the router gracefully
        session.router.shutdown().await?;
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
    // Parse the ticket
    let ticket: BlobTicket = ticket_string
        .parse()
        .context("Invalid ticket format")?;

    let output_path = PathBuf::from(&output_dir);
    if !output_path.exists() {
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

    // Initialize iroh endpoint for client
    let secret_key = SecretKey::generate(rand::rngs::OsRng);
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;

    // Create blob store and blobs handler
    let blobs = Blobs::persistent(temp_dir.path())
        .await
        .context("Failed to create blob store")?
        .build(&endpoint);

    // Emit progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Establishing connection...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 20.0,
    });

    // Get hash and node address from ticket
    let hash = ticket.hash();
    let node_addr = ticket.node_addr().clone();

    // Use the client to download
    let client = blobs.client();

    // Download the blob
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Downloading...".to_string(),
        bytes_transferred: 0,
        total_bytes: 0,
        percent: 40.0,
    });

    client
        .download(hash, node_addr)
        .await
        .context("Failed to start download")?
        .await
        .context("Download failed")?;

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

    // Export the blob to a file
    client
        .export(
            hash,
            final_path.clone(),
            iroh_blobs::store::ExportFormat::Blob,
            iroh_blobs::store::ExportMode::Copy,
        )
        .await
        .context("Failed to start export")?
        .await
        .context("Export failed")?;

    let file_size = std::fs::metadata(&final_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Emit complete
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Complete".to_string(),
        bytes_transferred: file_size,
        total_bytes: file_size,
        percent: 100.0,
    });

    // Cleanup
    endpoint.close().await;

    Ok(ReceiveResult {
        file_path: final_path.to_string_lossy().to_string(),
        file_name,
        file_size,
    })
}
