<template>
  <div class="send-view animate-fadeIn">
    <!-- File Selection Area -->
    <div v-if="!selectedItems.length" class="dropzone" @dragover.prevent="isDragging = true"
      @dragleave="isDragging = false" @drop.prevent="handleDrop" :class="{ active: isDragging }">
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
            <!-- Always show input for files that need naming -->
            <input 
              v-if="item.needsName || (showNameInput && index === 0)"
              v-model="item.name"
              class="name-input"
              :placeholder="'Enter filename ' + (index + 1) + ' (e.g., photo.jpg)'"
            />
            <h3 v-else @click="editFileName(index)">{{ item.name }}</h3>
            <p class="text-muted" v-if="item.size > 0">{{ formatFileSize(item.size) }}</p>
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
      <button v-if="!ticket && !isTransferring && !showNameInput" class="btn btn-primary btn-large w-full mt-lg" @click="startSend">
        <span>🚀</span> Start Sharing
      </button>
      
      <!-- Confirm Name Button -->
      <button v-if="showNameInput" class="btn btn-primary btn-large w-full mt-lg" @click="applyCustomName">
        <span>✓</span> Confirm Name & Share
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
        <button class="btn btn-secondary btn-icon" @click="copyTicket" :title="copied ? 'Copied!' : 'Copy'">
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
import { open } from '@tauri-apps/plugin-dialog';
import { basename } from '@tauri-apps/api/path';
import { readFile, stat } from '@tauri-apps/plugin-fs';

interface FileInfo {
  name: string;
  size: number;
}

interface TransferProgress {
  status: string;
  bytes_transferred: number;
  total_bytes: number;
  percent: number;
}

interface SendResult {
  ticket: string;
  file_name: string;
  file_size: number;
}

interface SelectedItem {
  name: string;
  path: string;
  size: number;
  isDir: boolean;
  needsName?: boolean;
  data?: Uint8Array;
}

const selectedItems = ref<SelectedItem[]>([]);
const isDragging = ref(false);
const isTransferring = ref(false);
const ticket = ref<string | null>(null);
const copied = ref(false);
const error = ref<string | null>(null);
const progress = ref<TransferProgress>({
  status: 'Initializing...',
  bytes_transferred: 0,
  total_bytes: 0,
  percent: 0
});
const showNameInput = ref(false);
const customFileName = ref('');

// Detect if running on Android
const isAndroid = /android/i.test(navigator.userAgent);

// Check if any files need naming
const needsNaming = computed(() => selectedItems.value.some(item => item.needsName));

const totalSize = computed(() => selectedItems.value.reduce((sum, item) => sum + item.size, 0));

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  // Listen for transfer progress events
  unlisten = await listen<TransferProgress>('transfer-progress', (event) => {
    progress.value = event.payload;
  });
});

onUnmounted(() => {
  if (unlisten) {
    unlisten();
  }
});

/**
 * Get file info using backend Rust command
 * Fast and doesn't freeze UI
 */
async function getFileInfo(filePath: string): Promise<FileInfo | null> {
  let name = "";
  let size = 0;

  try {
    // Rust backend get_file_info only works for standard file paths (not content://)
    if (!filePath.startsWith('content://')) {
      const info = await invoke<FileInfo>('get_file_info', { path: filePath });
      return info;
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
    
    // Regular file path - extract filename
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

  if (name) {
    return { name, size };
  }
  
  return null;
}

async function selectFiles() {
  try {
    // Use native file picker dialog with multiple file support
    const selected = await open({
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
  // Folder selection is not well-supported on Android
  if (isAndroid) {
    error.value = 'Folder selection is not supported on Android. Please select individual files.';
    return;
  }
  
  try {
    // Use native folder picker dialog
    const selected = await open({
      multiple: false,
      directory: true,
      title: 'Select a folder to send'
    });
    
    if (selected) {
      const folderPath = Array.isArray(selected) ? selected[0] : selected;
      const info = await getFileInfo(folderPath);
      const name = info?.name || 'folder';
      
      // For folders, we don't read the data here - the backend handles it
      selectedItems.value = [{
        name,
        path: folderPath,
        size: info?.size || 0, // Got exact size from backend
        isDir: true
      }];
    }
  } catch (e) {
    console.error('Folder selection error:', e);
    error.value = 'Failed to open folder picker. This may not be supported on your device.';
  }
}

async function processSelectedPaths(paths: string[]) {
  const items: SelectedItem[] = [];
  
  for (let i = 0; i < paths.length; i++) {
    const filePath = paths[i];
    const info = await getFileInfo(filePath);
    let name = info?.name;
    let size = info?.size || 0;
    let needsName = false;
    
    // If we couldn't extract a name, mark for manual input
    if (!name) {
      name = `File ${i + 1}`;
      needsName = true;
    }
    
    try {
      // Don't read the file data yet! Reading large files blocks the UI and IPC bridge.
      // We'll read it right before sending.
      items.push({
        name,
        path: filePath,
        size, // We have the exact size now!
        isDir: false,
        needsName
      });
    } catch (e) {
      console.error('Error adding file to selection:', e);
    }
  }
  
  selectedItems.value = items;
}

function handleDrop(event: DragEvent) {
  isDragging.value = false;
  // Handle dropped files - requires additional Tauri setup for mobile
  const files = event.dataTransfer?.files;
  if (files && files.length > 0) {
    console.log('Dropped files:', files);
  }
}

function clearSelection() {
  selectedItems.value = [];
  ticket.value = null;
  error.value = null;
  showNameInput.value = false;
  customFileName.value = '';
}

function editFileName(index: number) {
  if (index === 0 && selectedItems.value.length === 1) {
    customFileName.value = selectedItems.value[0].name;
    showNameInput.value = true;
  }
}

function applyCustomName() {
  if (customFileName.value.trim() && selectedItems.value.length > 0) {
    selectedItems.value[0].name = customFileName.value.trim();
  }
  showNameInput.value = false;
  // Auto-start send after confirming the name
  startSend();
}

async function startSend() {
  if (!selectedItems.value.length) return;

  isTransferring.value = true;
  error.value = null;

  try {
    let result: SendResult;
    const firstItem = selectedItems.value[0];
    
    // Check if this is a folder or if we have multiple items
    if (firstItem.isDir) {
      // Folder: use path-based send
      console.log('Sending folder:', firstItem.path);
      result = await invoke<SendResult>('start_send', {
        path: firstItem.path
      });
    } else if (selectedItems.value.length === 1 && isAndroid && firstItem.path.startsWith('content://')) {
      // Single Android content URI: must use bytes-based send
      progress.value.status = 'Reading file...';
      const fileData = await readFile(firstItem.path);
      console.log('Using start_send_bytes with', fileData.length, 'bytes');
      result = await invoke<SendResult>('start_send_bytes', {
        fileName: firstItem.name,
        data: Array.from(fileData)
      });
    } else if (selectedItems.value.length === 1) {
      // Single regular file: use path-based send
      console.log('Using start_send with path:', firstItem.path);
      result = await invoke<SendResult>('start_send', {
        path: firstItem.path
      });
    } else {
      // Multiple files: need to send paths array
      // For now, send first file - TODO: implement multi-file send command
      console.log('Multiple files - sending first file:', firstItem.path);
      if (isAndroid && firstItem.path.startsWith('content://')) {
        progress.value.status = 'Reading file...';
        const fileData = await readFile(firstItem.path);
        result = await invoke<SendResult>('start_send_bytes', {
          fileName: firstItem.name,
          data: Array.from(fileData)
        });
      } else {
        result = await invoke<SendResult>('start_send', {
          path: firstItem.path
        });
      }
    }

    ticket.value = result.ticket;
    isTransferring.value = false;
  } catch (e) {
    console.error('Send error:', e);
    error.value = String(e);
    isTransferring.value = false;
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

async function copyTicket() {
  if (!ticket.value) return;

  try {
    await navigator.clipboard.writeText(ticket.value);
    copied.value = true;
    setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch (e) {
    console.error('Copy error:', e);
  }
}

function resetState() {
  selectedItems.value = [];
  ticket.value = null;
  error.value = null;
  isTransferring.value = false;
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return 'Calculating...';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (bytes >= 1024 && i < units.length - 1) {
    bytes /= 1024;
    i++;
  }
  return `${bytes.toFixed(1)} ${units[i]}`;
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

.file-details h3 {
  cursor: pointer;
}

.file-details h3:hover {
  color: var(--accent-primary);
}

.text-small {
  font-size: 0.8rem;
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
