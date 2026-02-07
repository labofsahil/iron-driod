<template>
  <div class="send-view animate-fadeIn">
    <!-- File Selection Area -->
    <div v-if="!selectedFile" class="dropzone" @click="selectFile" @dragover.prevent="isDragging = true"
      @dragleave="isDragging = false" @drop.prevent="handleDrop" :class="{ active: isDragging }">
      <div class="dropzone-icon">📁</div>
      <h2>Select a File</h2>
      <p class="text-muted">Click to browse or drag & drop</p>
    </div>

    <!-- Selected File Info -->
    <div v-else class="card animate-slideUp">
      <div class="file-info">
        <div class="file-icon">📄</div>
        <div class="file-details">
          <h3>{{ selectedFile.name }}</h3>
          <p class="text-muted">{{ formatFileSize(selectedFile.size) }}</p>
        </div>
        <button class="btn btn-secondary btn-icon" @click="clearSelection" title="Remove">
          ✕
        </button>
      </div>

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
import { ref, onMounted, onUnmounted } from 'vue';
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

const selectedFile = ref<{ name: string; path: string; size: number; data?: Uint8Array } | null>(null);
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

async function selectFile() {
  try {
    // Use native file picker dialog
    const selected = await open({
      multiple: false,
      directory: false,
      title: 'Select a file to send'
    });
    
    if (selected) {
      console.log('Selected file path:', selected);
      // Get file name from path
      const name = selected.split('/').pop() || selected.split('\\').pop() || selected;
      
      // Try to read the file content immediately
      // This works for both regular paths and content:// URIs on Android
      try {
        console.log('Reading file content...');
        const data = await readFile(selected);
        console.log('File read successfully, size:', data.length);
        selectedFile.value = {
          name,
          path: selected,
          size: data.length,
          data: data
        };
      } catch (readErr) {
        console.error('Failed to read file, will try path-based send:', readErr);
        // Fallback: store without data, will try path-based send
        selectedFile.value = {
          name,
          path: selected,
          size: 0
        };
      }
    }
  } catch (e) {
    console.error('File selection error:', e);
    error.value = 'Failed to open file picker';
  }
}

function handleDrop(event: DragEvent) {
  isDragging.value = false;
  // Handle dropped files - requires additional Tauri setup for mobile
  const files = event.dataTransfer?.files;
  if (files && files.length > 0) {
    // This would need native path resolution
    console.log('Dropped files:', files);
  }
}

function clearSelection() {
  selectedFile.value = null;
  ticket.value = null;
  error.value = null;
}

async function startSend() {
  if (!selectedFile.value) return;

  isTransferring.value = true;
  error.value = null;

  try {
    let result: SendResult;
    
    if (selectedFile.value.data) {
      // Use bytes-based send (works with Android content:// URIs)
      console.log('Using start_send_bytes with', selectedFile.value.data.length, 'bytes');
      result = await invoke<SendResult>('start_send_bytes', {
        fileName: selectedFile.value.name,
        data: Array.from(selectedFile.value.data) // Convert Uint8Array to array for serialization
      });
    } else {
      // Fallback to path-based send
      console.log('Using start_send with path:', selectedFile.value.path);
      result = await invoke<SendResult>('start_send', {
        path: selectedFile.value.path
      });
    }

    ticket.value = result.ticket;
    selectedFile.value = {
      ...selectedFile.value,
      size: result.file_size
    };
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
  selectedFile.value = null;
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

.file-info {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
}

.file-icon {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  font-size: 24px;
}

.file-details {
  flex: 1;
  min-width: 0;
}

.file-details h3 {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
