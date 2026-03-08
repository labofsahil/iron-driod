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
          <div class="progress-bar">
            <div class="progress-fill" :style="{ width: progress.percent + '%' }"></div>
          </div>
          <div class="progress-text">
            <span>{{ progress.status }}</span>
            <span>{{ Math.round(progress.percent) }}%</span>
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

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  // Listen for receive-specific progress events (avoids collision with SendView)
  unlisten = await listen<TransferProgress>('receive-progress', (event) => {
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
</style>
