<script setup lang="ts">
import { ref } from 'vue';
import SendView from './components/SendView.vue';
import ReceiveView from './components/ReceiveView.vue';

type TabType = 'send' | 'receive';

const activeTab = ref<TabType>('send');

function setTab(tab: TabType) {
  activeTab.value = tab;
}
</script>

<template>
  <div id="iron-app">
    <!-- Header -->
    <header class="app-header">
      <div class="logo">
        <span class="logo-icon">⚡</span>
        <h1>Iron Send</h1>
      </div>
      <p class="tagline">Secure P2P File Transfer</p>
    </header>

    <!-- Tab Navigation -->
    <nav class="tabs">
      <button class="tab" :class="{ active: activeTab === 'send' }" @click="setTab('send')">
        <span class="tab-icon">📤</span> Send
      </button>
      <button class="tab" :class="{ active: activeTab === 'receive' }" @click="setTab('receive')">
        <span class="tab-icon">📥</span> Receive
      </button>
    </nav>

    <!-- Main Content -->
    <main class="app-content">
      <KeepAlive>
        <component :is="activeTab === 'send' ? SendView : ReceiveView" :key="activeTab" />
      </KeepAlive>
    </main>

    <!-- Footer -->
    <footer class="app-footer">
      <p>Powered by <a href="https://iroh.computer" target="_blank">iroh</a></p>
    </footer>
  </div>
</template>

<style>
@import './styles/main.css';

/* ========== App Layout ========== */
#iron-app {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
  max-width: 500px;
  margin: 0 auto;
  padding: var(--spacing-md);
  padding-bottom: env(safe-area-inset-bottom, var(--spacing-md));
}

/* ========== Header ========== */
.app-header {
  text-align: center;
  padding: var(--spacing-lg) 0;
  flex-shrink: 0;
}

.logo {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
}

.logo-icon {
  font-size: 2rem;
  background: var(--accent-gradient);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.logo h1 {
  font-size: 1.5rem;
  font-weight: 700;
  background: var(--accent-gradient);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.tagline {
  font-size: 0.875rem;
  color: var(--text-muted);
  margin-top: var(--spacing-xs);
}

/* ========== Navigation ========== */
.tabs {
  display: flex;
  background: var(--bg-secondary);
  padding: var(--spacing-xs);
  border-radius: var(--radius-lg);
  gap: var(--spacing-xs);
  flex-shrink: 0;
}

.tab {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-md) var(--spacing-sm);
  background: transparent;
  border: none;
  border-radius: var(--radius-md);
  font-family: inherit;
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all var(--transition-fast);
}

.tab:hover {
  color: var(--text-primary);
  background: var(--bg-glass);
}

.tab.active {
  color: white;
  background: var(--accent-gradient);
  box-shadow: var(--shadow-sm);
}

.tab-icon {
  font-size: 1rem;
}

/* ========== Main Content ========== */
.app-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  margin-top: var(--spacing-md);
}

/* ========== Footer ========== */
.app-footer {
  flex-shrink: 0;
  text-align: center;
  padding: var(--spacing-md) 0;
}

.app-footer p {
  font-size: 0.75rem;
  color: var(--text-muted);
}

.app-footer a {
  color: var(--accent-primary);
  text-decoration: none;
  font-weight: 500;
}

.app-footer a:hover {
  text-decoration: underline;
}

/* ========== Mobile Adjustments ========== */
@media (max-width: 400px) {
  #iron-app {
    padding: var(--spacing-sm);
  }

  .app-header {
    padding: var(--spacing-md) 0;
  }

  .logo h1 {
    font-size: 1.25rem;
  }

  .tab {
    padding: var(--spacing-sm);
    font-size: 0.8rem;
  }

  .tab-icon {
    font-size: 0.9rem;
  }
}
</style>