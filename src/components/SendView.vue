<template>
  <div class="send-view animate-fadeIn">
    <!-- File Selection Area -->
    <div v-if="!selectedItems.length" class="dropzone">
      <div class="dropzone-icon">📁</div>
      <h2>Select Files{{ isAndroid ? '' : ' or Folder' }}</h2>
      <p class="text-muted">{{ isAndroid ? 'Choose files to share' : 'Choose files or a folder to share' }}</p>
      
      <div class="selection-buttons mt-lg">
        <button class="btn btn-secondary" @click="selectFiles">
          <span>📄</span> Select Files
        </button>
        <button v-if="!isAndroid" class="btn btn-secondary" @click="selectFolder">
          <span>📁</span> Select Folder
        </button>
      </div>
    </div>

    <!-- Selected Files/Folder Info -->
    <div v-else class="card animate-slideUp">
      <!-- Android Naming Notice -->
      <div v-if="isAndroid && needsNaming" class="naming-notice">
        <span class="notice-icon">✏️</span>
        <p>Please enter the correct filename(s) with extension</p>
      </div>
      
      <div class="file-list">
        <div v-for="(item, index) in selectedItems" :key="index" class="file-info">
          <div class="file-icon">{{ item.isDir ? '📁' : '📄' }}</div>
          <div class="file-details">
            <!-- Show input for files that need naming -->
            <input 
              v-if="item.needsName"
              v-model="item.name"
              class="name-input"
              :placeholder="'Enter filename ' + (index + 1) + ' (e.g., photo.jpg)'"
            />
            <h3 v-else>{{ item.name }}</h3>
            <p class="text-muted" v-if="item.size > 0">{{ formatFileSize(item.size, 'Calculating...') }}</p>
            <p class="text-muted" v-else>Ready to send</p>
          </div>
        </div>
        
        <div v-if="selectedItems.length > 1" class="total-info">
          <strong>{{ selectedItems.length }} items</strong>
          <span class="text-muted" v-if="totalSize > 0">{{ formatFileSize(totalSize) }} total</span>
        </div>
      </div>
      
      <button class="btn btn-secondary btn-icon clear-btn" @click="clearSelection" title="Remove All">
        ✕ Clear
      </button>

      <!-- Progress Bar (during transfer) -->
      <div v-if="isTransferring" class="progress-container mt-lg">
        <div class="progress-bar">
          <div class="progress-fill" :style="{ width: progress.percent + '%' }"></div>
        </div>
        <div class="progress-text">
          <span>{{ progress.status }}</span>
          <span>{{ Math.round(progress.percent) }}%</span>
        </div>
      </div>

      <!-- Send Button -->
      <button v-if="!ticket && !isTransferring" class="btn btn-primary btn-large w-full mt-lg" @click="startSend" :disabled="!isReadyToSend">
        <span>🚀</span> {{ isLoadingMetadata ? 'Loading...' : 'Start Sharing' }}
      </button>
    </div>

    <!-- Generated Ticket -->
    <div v-if="ticket" class="card card-glow mt-lg animate-slideUp">
      <div class="flex flex-between" style="align-items: center; margin-bottom: 1rem;">
        <h3>Share This Ticket</h3>
        <span class="badge badge-success">Ready to Send</span>
      </div>

      <div class="ticket-display">
        <span class="ticket-text">{{ ticket }}</span>
        <button class="btn btn-secondary btn-icon" @click="handleCopy" :title="copied ? 'Copied!' : 'Copy'">
          {{ copied ? '✓' : '📋' }}
        </button>
      </div>

      <p class="text-muted text-center mt-md" style="font-size: 0.875rem;">
        Share this ticket with the recipient. Keep this app open until transfer completes.
      </p>

      <button class="btn btn-secondary w-full mt-lg" @click="cancelSend">
        Cancel Transfer
      </button>
    </div>

    <!-- Error Display -->
    <div v-if="error" class="card mt-lg" style="border-color: var(--error);">
      <div class="flex gap-sm" style="align-items: center;">
        <span style="font-size: 1.5rem;">⚠️</span>
        <div>
          <h4 style="color: var(--error);">Error</h4>
          <p class="text-muted">{{ error }}</p>
        </div>
      </div>
      <button class="btn btn-secondary w-full mt-md" @click="resetState">Try Again</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { basename, join, appCacheDir } from '@tauri-apps/api/path';
import { stat, mkdir, remove, BaseDirectory, open as openFile } from '@tauri-apps/plugin-fs';
import {
  type TransferProgress,
  type SendResult,
  type FileInfo,
  type SelectedItem,
  formatFileSize,
  copyToClipboard,
  defaultProgress,
  isAndroid,
} from '../utils';

const selectedItems = ref<SelectedItem[]>([]);
const isTransferring = ref(false);
const ticket = ref<string | null>(null);
const copied = ref(false);
const error = ref<string | null>(null);
const progress = ref<TransferProgress>(defaultProgress());

// Check if any files need naming
const needsNaming = computed(() => selectedItems.value.some(item => item.needsName));
const totalSize = computed(() => selectedItems.value.reduce((sum, item) => sum + item.size, 0));

// Track whether metadata is still loading for any item
const isLoadingMetadata = computed(() => selectedItems.value.some(item => item.loadingMetadata));

// CTA is only enabled when all metadata has settled and no items still need naming
const isReadyToSend = computed(() => {
  if (selectedItems.value.length === 0) return false;
  return selectedItems.value.every(item => {
    if (item.loadingMetadata) return false;
    if (item.needsName) {
      // For items requiring manual naming, ensure name is non-empty and path-safe
      return item.name.trim().length > 0 && !/[/\\:*?"<>|]/.test(item.name.trim());
    }
    return true;
  });
});

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  // Listen for send-specific progress events (avoids collision with ReceiveView)
  unlisten = await listen<TransferProgress>('send-progress', (event) => {
    progress.value = event.payload;
  });
});

onUnmounted(() => {
  unlisten?.();
});

/**
 * Get file info using backend Rust command.
 * Falls back to path parsing + Tauri stat for Android content:// URIs.
 */
async function getFileInfo(filePath: string): Promise<FileInfo | null> {
  let name = "";
  let size = 0;

  try {
    // Rust backend get_file_info only works for standard file paths (not content://)
    if (!filePath.startsWith('content://')) {
      return await invoke<FileInfo>('get_file_info', { path: filePath });
    }
  } catch (e) {
    console.error('Failed to get file info from Rust:', e);
  }

  // Fallback for Android content:// URIs
  try {
    const base = await basename(filePath);
    if (base) {
      name = decodeURIComponent(base);
    }
  } catch (e) {
    console.error('Failed to get basename:', e);
  }
  
  if (!name) {
    let decoded = filePath;
    try {
      decoded = decodeURIComponent(filePath);
    } catch {
      // If decoding fails, use as-is
    }
    
    const parts = decoded.split(/[/\\]/);
    const extracted = parts[parts.length - 1];
    if (extracted && !extracted.startsWith('content:')) {
      name = extracted;
    }
  }

  // Try to get size from Tauri's stat API which might support content:// on Android
  try {
    const fileStat = await stat(filePath);
    size = fileStat.size || 0;
  } catch(e) {
    console.warn('Could not stat file size:', e);
  }

  return name ? { name, size } : null;
}

async function selectFiles() {
  try {
    const selected = await openDialog({
      multiple: true,
      directory: false,
      title: 'Select files to send'
    });
    
    if (selected) {
      const paths = Array.isArray(selected) ? selected : [selected];
      await processSelectedPaths(paths);
    }
  } catch (e) {
    console.error('File selection error:', e);
    error.value = 'Failed to open file picker';
  }
}

async function selectFolder() {
  if (isAndroid) {
    error.value = 'Folder selection is not supported on Android. Please select individual files.';
    return;
  }
  
  try {
    const selected = await openDialog({
      multiple: false,
      directory: true,
      title: 'Select a folder to send'
    });
    
    if (selected) {
      const folderPath = Array.isArray(selected) ? selected[0] : selected;
      const info = await getFileInfo(folderPath);
      const name = info?.name || 'folder';
      
      selectedItems.value = [{
        name,
        path: folderPath,
        size: info?.size || 0,
        isDir: true
      }];
    }
  } catch (e) {
    console.error('Folder selection error:', e);
    error.value = 'Failed to open folder picker. This may not be supported on your device.';
  }
}

async function processSelectedPaths(paths: string[]) {
  // Immediately show files in UI with placeholder names for instant feedback
  const items: SelectedItem[] = paths.map((filePath, i) => {
    let tempName = `File ${i + 1}`;
    try {
      const decoded = decodeURIComponent(filePath);
      const parts = decoded.split(/[/\\]/);
      const extracted = parts[parts.length - 1];
      if (extracted && !extracted.startsWith('content:')) {
        tempName = extracted;
      }
    } catch {}

    return {
      name: tempName,
      path: filePath,
      size: 0,
      isDir: false,
      needsName: false,
      loadingMetadata: true,
    };
  });
  
  selectedItems.value = items;

  // Fetch proper file info asynchronously without blocking UI
  for (let i = 0; i < selectedItems.value.length; i++) {
    const item = selectedItems.value[i];
    
    getFileInfo(item.path).then(info => {
      if (info?.name) {
        item.name = info.name;
      } else {
        item.needsName = true;
      }
      if (info?.size) {
        item.size = info.size;
      }
    }).catch(e => {
      console.error('Error fetching file info:', e);
      item.needsName = true;
    }).finally(() => {
      item.loadingMetadata = false;
    });
  }
}

function clearSelection() {
  selectedItems.value = [];
  ticket.value = null;
  error.value = null;
}

/**
 * Stream a content:// URI to a local cache file in 1MB chunks.
 * Avoids loading the entire file into JS memory (V8 array length limit).
 * Returns the absolute path to the cached file.
 */
async function cacheContentUri(contentUri: string, fileName: string): Promise<string> {
  const CHUNK_SIZE = 1024 * 1024; // 1MB chunks
  const cacheDir = await appCacheDir();
  const batchDirName = 'iron_send_' + Date.now();
  await mkdir(batchDirName, { baseDir: BaseDirectory.AppCache });
  const relativeFilePath = `${batchDirName}/${fileName}`;

  const srcFile = await openFile(contentUri, { read: true });
  const dstFile = await openFile(relativeFilePath, { write: true, create: true, baseDir: BaseDirectory.AppCache });

  let errorOccurred = false;
  try {
    const buf = new Uint8Array(CHUNK_SIZE);
    let totalWritten = 0;
    while (true) {
      const bytesRead = await srcFile.read(buf);
      if (bytesRead === null) break;
      const chunk = bytesRead < CHUNK_SIZE ? buf.subarray(0, bytesRead) : buf;
      await dstFile.write(chunk);
      totalWritten += bytesRead;
      progress.value.status = `Caching... ${(totalWritten / (1024 * 1024)).toFixed(0)} MB`;
    }
  } catch (e) {
    errorOccurred = true;
    throw e;
  } finally {
    // Always close file handles
    try { await srcFile.close(); } catch { /* ignore close errors */ }
    try { await dstFile.close(); } catch { /* ignore close errors */ }
    // Remove partial cache file if an error occurred
    if (errorOccurred) {
      try { await remove(relativeFilePath, { baseDir: BaseDirectory.AppCache }); } catch { /* best effort */ }
    }
  }

  return await join(cacheDir, relativeFilePath);
}

async function startSend() {
  if (!selectedItems.value.length) return;

  isTransferring.value = true;
  error.value = null;

  // Track cached paths for cleanup after send
  const cachedPaths: string[] = [];

  try {
    let result: SendResult;
    const firstItem = selectedItems.value[0];
    
    if (firstItem.isDir) {
      // Folder: use path-based send
      result = await invoke<SendResult>('start_send', { path: firstItem.path });
    } else if (selectedItems.value.length === 1 && isAndroid && firstItem.path.startsWith('content://')) {
      // Stream content:// URI to local cache first
      progress.value.status = 'Caching file to disk...';
      const cachedPath = await cacheContentUri(firstItem.path, firstItem.name);
      cachedPaths.push(cachedPath);
      result = await invoke<SendResult>('start_send', { path: cachedPath });
    } else if (selectedItems.value.length === 1) {
      // Single regular file
      result = await invoke<SendResult>('start_send', { path: firstItem.path });
    } else {
      // Multiple files
      const hasContentUri = isAndroid && selectedItems.value.some(item => item.path.startsWith('content://'));
      
      if (hasContentUri) {
        progress.value.status = 'Caching files...';
        const paths: string[] = [];
        for (const item of selectedItems.value) {
           if (item.path.startsWith('content://')) {
             progress.value.status = 'Caching ' + item.name;
             const cachedPath = await cacheContentUri(item.path, item.name);
             cachedPaths.push(cachedPath);
             paths.push(cachedPath);
           } else {
             paths.push(item.path);
           }
        }
        
        progress.value.status = 'Starting transfer...';
        result = await invoke<SendResult>('start_send_multiple', { paths });
      } else {
        const paths = selectedItems.value.map(item => item.path);
        result = await invoke<SendResult>('start_send_multiple', { paths });
      }
    }

    ticket.value = result.ticket;
    isTransferring.value = false;
  } catch (e) {
    console.error('Send error:', e);
    error.value = String(e);
    isTransferring.value = false;
  } finally {
    // Clean up cached content:// files (best effort)
    for (const cached of cachedPaths) {
      try {
        await remove(cached);
      } catch {
        console.warn('Failed to clean cached file:', cached);
      }
    }
  }
}

async function cancelSend() {
  try {
    await invoke('cancel_send');
    ticket.value = null;
    isTransferring.value = false;
  } catch (e) {
    console.error('Cancel error:', e);
  }
}

async function handleCopy() {
  if (!ticket.value) return;
  const ok = await copyToClipboard(ticket.value);
  if (ok) {
    copied.value = true;
    setTimeout(() => { copied.value = false; }, 2000);
  }
}

function resetState() {
  selectedItems.value = [];
  ticket.value = null;
  error.value = null;
  isTransferring.value = false;
}
</script>

<style scoped>
.send-view {
  flex: 1;
  padding: var(--spacing-lg);
  overflow-y: auto;
}

.selection-buttons {
  display: flex;
  gap: var(--spacing-md);
  justify-content: center;
}

.selection-buttons .btn {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.file-list {
  max-height: 200px;
  overflow-y: auto;
  margin-bottom: var(--spacing-md);
}

.file-info {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  padding: var(--spacing-sm) 0;
  border-bottom: 1px solid var(--bg-tertiary);
}

.file-info:last-of-type {
  border-bottom: none;
}

.file-icon {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  font-size: 20px;
  flex-shrink: 0;
}

.file-details {
  flex: 1;
  min-width: 0;
}

.file-details h3 {
  font-size: 0.9rem;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.file-details p {
  font-size: 0.75rem;
}

.total-info {
  display: flex;
  justify-content: space-between;
  padding: var(--spacing-md) 0;
  border-top: 1px solid var(--bg-glass);
  margin-top: var(--spacing-sm);
}

.clear-btn {
  width: 100%;
  margin-bottom: var(--spacing-md);
}

.name-input {
  width: 100%;
  padding: var(--spacing-sm);
  font-size: 0.9rem;
  border: 1px solid var(--accent-primary);
  border-radius: var(--radius-sm);
  background: var(--bg-secondary);
  color: var(--text-primary);
}

.name-input:focus {
  outline: none;
  border-color: var(--accent-secondary);
  box-shadow: 0 0 0 2px rgba(var(--accent-primary-rgb), 0.2);
}

.naming-notice {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-md);
  margin-bottom: var(--spacing-md);
  background: linear-gradient(135deg, rgba(255, 193, 7, 0.1), rgba(255, 152, 0, 0.1));
  border: 1px solid rgba(255, 193, 7, 0.3);
  border-radius: var(--radius-md);
}

.naming-notice .notice-icon {
  font-size: 1.5rem;
}

.naming-notice p {
  margin: 0;
  font-size: 0.85rem;
  color: var(--text-primary);
}
</style>
