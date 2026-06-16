<template>
  <div class="receive-view animate-fadeIn">
    <!-- Ticket Input -->
    <div v-if="!isReceiving && !result" class="card">
      <h2 class="text-center" style="margin-bottom: var(--spacing-lg);">Receive a File</h2>

      <div class="input-group">
        <label class="input-label">Paste Ticket</label>
        <textarea v-model="ticketInput" class="input input-textarea"
          placeholder="Paste the ticket here. It starts with 'blob:...'"></textarea>
      </div>

      <button class="btn btn-primary btn-large w-full mt-lg" @click="startReceive" :disabled="!ticketInput.trim()">
        <span>📥</span> Start Download
      </button>
    </div>

    <!-- Receiving Progress -->
    <div v-if="isReceiving" class="card animate-slideUp">
      <div class="flex flex-center flex-col gap-lg" style="padding: var(--spacing-xl) 0;">
        <div class="receiving-animation">
          <div class="receiving-icon animate-pulse">📥</div>
        </div>

        <h2>Receiving File...</h2>

        <div class="progress-container w-full">
          <div class="progress-bar" :class="{ indeterminate: progress.total_bytes === 0 }">
            <div class="progress-fill" :style="{ width: (progress.total_bytes > 0 ? progress.percent : 100) + '%' }"></div>
          </div>
          
          <div class="progress-details mt-md w-full">
            <div class="detail-row">
              <span class="detail-label">Status</span>
              <span class="detail-value">{{ progress.status }}</span>
            </div>
            <div class="detail-row">
              <span class="detail-label">Received</span>
              <span class="detail-value font-mono">{{ formatFileSize(progress.bytes_transferred) }}</span>
            </div>
            <div v-if="progress.total_bytes > 0" class="detail-row">
              <span class="detail-label">Total Size</span>
              <span class="detail-value font-mono">{{ formatFileSize(progress.total_bytes) }}</span>
            </div>
            <div class="detail-row">
              <span class="detail-label">Speed</span>
              <span class="detail-value speed-val">{{ currentSpeed }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Success Result -->
    <div v-if="result" class="card card-glow animate-slideUp">
      <div class="flex flex-center flex-col gap-lg" style="padding: var(--spacing-lg) 0;">
        <div class="success-icon">✅</div>
        <h2>Download Complete!</h2>

        <div class="file-info-card">
          <div class="file-icon">📄</div>
          <div class="file-details">
            <h3>{{ result.file_name }}</h3>
            <p class="text-muted">{{ formatFileSize(result.file_size) }}</p>
          </div>
        </div>

        <p class="text-muted text-center">
          Saved to: <span class="text-mono">{{ result.file_path }}</span>
        </p>

        <button class="btn btn-secondary w-full" @click="resetState">
          Receive Another File
        </button>
      </div>
    </div>

    <!-- Error Display -->
    <div v-if="error" class="card mt-lg" style="border-color: var(--error);">
      <div class="flex gap-sm" style="align-items: center;">
        <span style="font-size: 1.5rem;">⚠️</span>
        <div>
          <h4 style="color: var(--error);">Download Failed</h4>
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
import {
  type TransferProgress,
  type ReceiveResult,
  formatFileSize,
  defaultProgress,
} from '../utils';

const ticketInput = ref('');
const isReceiving = ref(false);
const result = ref<ReceiveResult | null>(null);
const error = ref<string | null>(null);
const progress = ref<TransferProgress>(defaultProgress('Connecting...'));

const startTime = ref<number | null>(null);
const lastUpdateTime = ref<number | null>(null);
const lastBytesTransferred = ref<number>(0);
const currentSpeed = ref<string>('0 B/s');

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  // Listen for receive-specific progress events (avoids collision with SendView)
  unlisten = await listen<TransferProgress>('receive-progress', (event) => {
    const now = Date.now();
    const bytes = event.payload.bytes_transferred;
    
    if (startTime.value === null) {
      startTime.value = now;
      lastUpdateTime.value = now;
      lastBytesTransferred.value = bytes;
    } else {
      const timeDelta = now - (lastUpdateTime.value || now);
      if (timeDelta >= 500) {
        const bytesDelta = bytes - lastBytesTransferred.value;
        const speedBytesPerSec = (bytesDelta / timeDelta) * 1000;
        currentSpeed.value = `${formatFileSize(speedBytesPerSec)}/s`;
        lastUpdateTime.value = now;
        lastBytesTransferred.value = bytes;
      }
    }

    progress.value = event.payload;
  });
});

onUnmounted(() => {
  unlisten?.();
});

async function startReceive() {
  if (!ticketInput.value.trim()) return;

  isReceiving.value = true;
  error.value = null;
  startTime.value = null;
  lastUpdateTime.value = null;
  lastBytesTransferred.value = 0;
  currentSpeed.value = '0 B/s';

  try {
    const outputDir = await invoke<string>('get_downloads_dir');

    const receiveResult = await invoke<ReceiveResult>('receive_file', {
      ticket: ticketInput.value.trim(),
      outputDir
    });

    result.value = receiveResult;
    isReceiving.value = false;
  } catch (e) {
    console.error('Receive error:', e);
    error.value = String(e);
    isReceiving.value = false;
  }
}

function resetState() {
  ticketInput.value = '';
  isReceiving.value = false;
  result.value = null;
  error.value = null;
  progress.value = defaultProgress('Connecting...');
  startTime.value = null;
  lastUpdateTime.value = null;
  lastBytesTransferred.value = 0;
  currentSpeed.value = '0 B/s';
}
</script>

<style scoped>
.receive-view {
  flex: 1;
  padding: var(--spacing-lg);
  overflow-y: auto;
}

.receiving-animation {
  width: 80px;
  height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-gradient);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-glow);
}

.receiving-icon {
  font-size: 36px;
}

.success-icon {
  font-size: 64px;
}

.file-info-card {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  padding: var(--spacing-md);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  width: 100%;
}

.file-icon {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-secondary);
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

.progress-details {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  padding: var(--spacing-md);
  margin-top: var(--spacing-md);
  border: 1px solid var(--border-color);
}

.detail-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 0.85rem;
}

.detail-label {
  color: var(--text-muted);
}

.detail-value {
  color: var(--text-primary);
  font-weight: 500;
}

.speed-val {
  color: var(--success);
  font-weight: 600;
}

.font-mono {
  font-family: var(--font-mono);
}
</style>
