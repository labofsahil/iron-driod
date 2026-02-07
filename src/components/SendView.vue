<template>
  <div class="send-view animate-fadeIn">
    <!-- File Selection Area -->
    <div v-if="!selectedItems.length" class="dropzone" @dragover.prevent="isDragging = true"
      @dragleave="isDragging = false" @drop.prevent="handleDrop" :class="{ active: isDragging }">
      <div class="dropzone-icon">📁</div>
      <h2>Select Files or Folder</h2>
      <p class="text-muted">Choose files or a folder to share</p>
      
      <div class="selection-buttons mt-lg">
        <button class="btn btn-secondary" @click="selectFiles">
          <span>📄</span> Select Files
        </button>
        <button class="btn btn-secondary" @click="selectFolder">
          <span>📁</span> Select Folder
        </button>
      </div>
    </div>

    <!-- Selected Files/Folder Info -->
    <div v-else class="card animate-slideUp">
      <div class="file-list">
        <div v-for="(item, index) in selectedItems" :key="index" class="file-info">
          <div class="file-icon">{{ item.isDir ? '📁' : '📄' }}</div>
          <div class="file-details">
            <h3>{{ item.name }}</h3>
            <p class="text-muted">{{ formatFileSize(item.size) }}</p>
          </div>
        </div>
        
        <div v-if="selectedItems.length > 1" class="total-info">
          <strong>{{ selectedItems.length }} items</strong>
          <span class="text-muted">{{ formatFileSize(totalSize) }} total</span>
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
      <button v-if="!ticket && !isTransferring" class="btn btn-primary btn-large w-full mt-lg" @click="startSend">
        <span>🚀</span> Start Sharing
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
import { readFile } from '@tauri-apps/plugin-fs';

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
      await processSelectedPaths(paths, false);
    }
  } catch (e) {
    console.error('File selection error:', e);
    error.value = 'Failed to open file picker';
  }
}

async function selectFolder() {
  try {
    // Use native folder picker dialog
    const selected = await open({
      multiple: false,
      directory: true,
      title: 'Select a folder to send'
    });
    
    if (selected) {
      const folderPath = Array.isArray(selected) ? selected[0] : selected;
      const name = folderPath.split('/').pop() || folderPath.split('\\').pop() || folderPath;
      
      // For folders, we don't read the data here - the backend handles it
      selectedItems.value = [{
        name,
        path: folderPath,
        size: 0, // Will be calculated by backend
        isDir: true
      }];
    }
  } catch (e) {
    console.error('Folder selection error:', e);
    error.value = 'Failed to open folder picker';
  }
}

async function processSelectedPaths(paths: string[], isDir: boolean) {
  const items: SelectedItem[] = [];
  
  for (const filePath of paths) {
    const name = filePath.split('/').pop() || filePath.split('\\').pop() || filePath;
    
    try {
      console.log('Reading file:', filePath);
      const data = await readFile(filePath);
      console.log('File read successfully, size:', data.length);
      items.push({
        name,
        path: filePath,
        size: data.length,
        isDir: false,
        data: data
      });
    } catch (readErr) {
      console.error('Failed to read file:', readErr);
      // Fallback: store without data
      items.push({
        name,
        path: filePath,
        size: 0,
        isDir: isDir
      });
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
    } else if (selectedItems.value.length === 1 && firstItem.data) {
      // Single file with data: use bytes-based send
      console.log('Using start_send_bytes with', firstItem.data.length, 'bytes');
      result = await invoke<SendResult>('start_send_bytes', {
        fileName: firstItem.name,
        data: Array.from(firstItem.data)
      });
    } else if (selectedItems.value.length === 1) {
      // Single file without data: use path-based send
      console.log('Using start_send with path:', firstItem.path);
      result = await invoke<SendResult>('start_send', {
        path: firstItem.path
      });
    } else {
      // Multiple files: need to send paths array
      // For now, send first file - TODO: implement multi-file send command
      console.log('Multiple files - sending first file:', firstItem.path);
      if (firstItem.data) {
        result = await invoke<SendResult>('start_send_bytes', {
          fileName: firstItem.name,
          data: Array.from(firstItem.data)
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
</style>
