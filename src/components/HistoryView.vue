<template>
  <div class="history-view animate-fadeIn">
    <div v-if="history.length === 0" class="empty-state">
      <div class="empty-icon">📋</div>
      <h2>No Transfer History</h2>
      <p class="text-muted">Your recent file transfers will appear here</p>
    </div>

    <div v-else>
      <div class="flex flex-between" style="align-items: center; margin-bottom: var(--spacing-lg);">
        <h2>Recent Transfers</h2>
        <button class="btn btn-secondary btn-icon" @click="clearHistory" title="Clear History">
          🗑️
        </button>
      </div>

      <div class="history-list">
        <div v-for="item in history" :key="item.id" class="history-item card animate-fadeIn">
          <div class="history-icon" :class="item.type">
            {{ item.type === 'sent' ? '📤' : '📥' }}
          </div>

          <div class="history-details">
            <h3>{{ item.fileName }}</h3>
            <p class="text-muted">{{ formatFileSize(item.fileSize) }} • {{ formatDate(item.timestamp) }}</p>
          </div>

          <span class="badge" :class="item.type === 'sent' ? 'badge-pending' : 'badge-success'">
            {{ item.type === 'sent' ? 'Sent' : 'Received' }}
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';

interface HistoryItem {
  id: string;
  type: 'sent' | 'received';
  fileName: string;
  fileSize: number;
  timestamp: number;
  ticket?: string;
  filePath?: string;
}

const history = ref<HistoryItem[]>([]);

onMounted(() => {
  loadHistory();
});

function loadHistory() {
  try {
    const saved = localStorage.getItem('iron-transfer-history');
    if (saved) {
      history.value = JSON.parse(saved);
    }
  } catch (e) {
    console.error('Failed to load history:', e);
  }
}

function clearHistory() {
  if (confirm('Clear all transfer history?')) {
    history.value = [];
    localStorage.removeItem('iron-transfer-history');
  }
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return 'Unknown';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (bytes >= 1024 && i < units.length - 1) {
    bytes /= 1024;
    i++;
  }
  return `${bytes.toFixed(1)} ${units[i]}`;
}

function formatDate(timestamp: number): string {
  const date = new Date(timestamp);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return 'Just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;

  return date.toLocaleDateString();
}
</script>

<style scoped>
.history-view {
  flex: 1;
  padding: var(--spacing-lg);
  overflow-y: auto;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 60%;
  text-align: center;
  gap: var(--spacing-md);
}

.empty-icon {
  font-size: 64px;
  opacity: 0.5;
}

.history-list {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}

.history-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  padding: var(--spacing-md);
}

.history-icon {
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  font-size: 20px;
}

.history-icon.sent {
  background: rgba(99, 102, 241, 0.15);
}

.history-icon.received {
  background: rgba(16, 185, 129, 0.15);
}

.history-details {
  flex: 1;
  min-width: 0;
}

.history-details h3 {
  font-size: 0.95rem;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.history-details p {
  font-size: 0.8rem;
}
</style>
