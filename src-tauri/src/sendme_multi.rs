
/// Start sending multiple files by path
pub async fn start_send_multiple(
    state: &SendmeState,
    paths: Vec<String>,
    app: AppHandle,
) -> Result<SendResult> {
    info!("start_send_multiple called with {} paths", paths.len());
    
    if paths.is_empty() {
        return Err(anyhow!("No paths provided"));
    }

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

    // Emit initial progress
    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Initializing...".to_string(),
        bytes_transferred: 0,
        total_bytes: total_size,
        percent: 0.0,
    });

    let temp_dir = tempfile::Builder::new().prefix(".iron-send-").tempdir().context("Failed to create temp directory")?;
    
    let secret_key = SecretKey::generate(&mut rand::rng());
    let endpoint = Endpoint::builder().secret_key(secret_key).relay_mode(RelayMode::Default).bind().await.context("Failed to create endpoint")?;

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Setting up connection...".to_string(),
        bytes_transferred: 0,
        total_bytes: total_size,
        percent: 10.0,
    });

    let store = FsStore::load(temp_dir.path()).await.context("Failed to load store")?;
    let blobs = BlobsProtocol::new(&store, None);
    let client = store.blobs();

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Importing files...".to_string(),
        bytes_transferred: 0,
        total_bytes: total_size,
        percent: 20.0,
    });

    let mut collection = Collection::default();
    
    for path_str in paths {
        let path = PathBuf::from(&path_str);
        if path.is_file() {
            let file_name = get_name(&path);
            let progress = client.add_path(&path);
            let add_outcome = progress.await.context("Failed to add file")?;
            collection.push(file_name, add_outcome.hash);
        } else {
            let base_path = path.canonicalize().context("Failed to canonicalize path")?;
            for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    let file_path = entry.path();
                    let mut relative_path = file_path.strip_prefix(&base_path).unwrap_or(file_path).to_string_lossy().to_string();
                    let folder_name = get_name(&path);
                    relative_path = format!("{}/{}", folder_name, relative_path);
                    let progress = client.add_path(file_path);
                    let add_outcome = progress.await.context("Failed to add file")?;
                    collection.push(relative_path, add_outcome.hash);
                }
            }
        }
    }

    let collection_tag = collection.store(store.as_ref()).await.context("Failed to store collection")?;
    let collection_hash = collection_tag.hash();

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Starting server...".to_string(),
        bytes_transferred: total_size,
        total_bytes: total_size,
        percent: 60.0,
    });

    let router = Router::builder(endpoint).accept(iroh_blobs::ALPN, blobs).spawn();
    router.endpoint().online().await;

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Generating ticket...".to_string(),
        bytes_transferred: total_size,
        total_bytes: total_size,
        percent: 80.0,
    });

    let ticket = BlobTicket::new(router.endpoint().addr(), collection_hash, BlobFormat::HashSeq);
    let ticket_string = ticket.to_string();

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Ready to send".to_string(),
        bytes_transferred: total_size,
        total_bytes: total_size,
        percent: 100.0,
    });

    let mut active = state.active_send.lock().await;
    *active = Some(SendSession { router, _temp_dir: temp_dir });

    Ok(SendResult {
        ticket: ticket_string,
        file_name: display_name,
        file_size: total_size,
    })
}

/// Start sending multiple raw byte files (for Android content:// URIs)
pub async fn start_send_multiple_bytes(
    state: &SendmeState,
    files: Vec<FileDataPayload>,
    app: AppHandle,
) -> Result<SendResult> {
    info!("start_send_multiple_bytes called with {} files", files.len());
    
    if files.is_empty() {
        return Err(anyhow!("No files provided"));
    }

    let total_size: u64 = files.iter().map(|f| f.data.len() as u64).sum();
    let display_name = if files.len() == 1 { files[0].file_name.clone() } else { format!("{} items", files.len()) };

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Initializing...".to_string(),
        bytes_transferred: 0,
        total_bytes: total_size,
        percent: 0.0,
    });

    let temp_dir = tempfile::Builder::new().prefix(".iron-send-").tempdir().context("Failed to create temp directory")?;
    let secret_key = SecretKey::generate(&mut rand::rng());
    let endpoint = Endpoint::builder().secret_key(secret_key).relay_mode(RelayMode::Default).bind().await.context("Failed to create endpoint")?;

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Setting up connection...".to_string(),
        bytes_transferred: 0,
        total_bytes: total_size,
        percent: 10.0,
    });

    let store = FsStore::load(temp_dir.path()).await.context("Failed to create blob store")?;
    let blobs = BlobsProtocol::new(&store, None);
    let client = store.blobs();

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Importing files...".to_string(),
        bytes_transferred: 0,
        total_bytes: total_size,
        percent: 20.0,
    });

    let mut collection = Collection::default();
    
    for file in files {
        let add_outcome = client.add_bytes(file.data).await.context("Failed to add bytes")?;
        collection.push(file.file_name, add_outcome.hash);
    }

    let collection_tag = collection.store(store.as_ref()).await.context("Failed to store collection")?;
    let collection_hash = collection_tag.hash();

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Starting server...".to_string(),
        bytes_transferred: total_size,
        total_bytes: total_size,
        percent: 60.0,
    });

    let router = Router::builder(endpoint).accept(iroh_blobs::ALPN, blobs).spawn();
    router.endpoint().online().await;

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Generating ticket...".to_string(),
        bytes_transferred: total_size,
        total_bytes: total_size,
        percent: 80.0,
    });

    let ticket = BlobTicket::new(router.endpoint().addr(), collection_hash, BlobFormat::HashSeq);
    let ticket_string = ticket.to_string();

    let _ = app.emit("transfer-progress", TransferProgress {
        status: "Ready to send".to_string(),
        bytes_transferred: total_size,
        total_bytes: total_size,
        percent: 100.0,
    });

    let mut active = state.active_send.lock().await;
    *active = Some(SendSession { router, _temp_dir: temp_dir });

    Ok(SendResult {
        ticket: ticket_string,
        file_name: display_name,
        file_size: total_size,
    })
}
