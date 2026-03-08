/**
 * Shared utilities and type definitions for Iron Send.
 */

// ─── Interfaces ──────────────────────────────────────────────────

/** Transfer progress event payload emitted from the Rust backend. */
export interface TransferProgress {
  status: string;
  bytes_transferred: number;
  total_bytes: number;
  percent: number;
}

/** Result returned by the `start_send` / `start_send_multiple` commands. */
export interface SendResult {
  ticket: string;
  file_name: string;
  file_size: number;
}

/** Result returned by the `receive_file` command. */
export interface ReceiveResult {
  file_path: string;
  file_name: string;
  file_size: number;
}

/** Basic file information returned by the `get_file_info` command. */
export interface FileInfo {
  name: string;
  size: number;
}

/** An item selected by the user for sending. */
export interface SelectedItem {
  name: string;
  path: string;
  size: number;
  isDir: boolean;
  needsName?: boolean;
  loadingMetadata?: boolean;
  data?: Uint8Array;
}

// ─── Helpers ─────────────────────────────────────────────────────

/**
 * Format a byte count into a human-readable string (e.g. "1.2 MB").
 * Returns `fallback` (default "Unknown size") when `bytes` is null, undefined, or NaN.
 * A value of 0 renders as "0.0 B".
 */
export function formatFileSize(bytes: number, fallback = 'Unknown size'): string {
  if (bytes == null || Number.isNaN(bytes) || bytes < 0) return fallback;
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (bytes >= 1024 && i < units.length - 1) {
    bytes /= 1024;
    i++;
  }
  return `${bytes.toFixed(1)} ${units[i]}`;
}

/**
 * Copy text to the clipboard with a fallback for environments where
 * `navigator.clipboard` is unavailable (e.g. Android WebView).
 * Returns `true` on success.
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  // Primary: modern Clipboard API
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // fall through to legacy method
    }
  }

  // Fallback: execCommand('copy') via a temporary textarea
  try {
    const textarea = document.createElement('textarea');
    textarea.value = text;
    textarea.style.position = 'fixed';
    textarea.style.left = '-9999px';
    textarea.style.top = '-9999px';
    textarea.style.opacity = '0';
    document.body.appendChild(textarea);
    textarea.focus();
    textarea.select();
    const ok = document.execCommand('copy');
    document.body.removeChild(textarea);
    return ok;
  } catch {
    return false;
  }
}

/** Detect whether the app is running on Android. */
export const isAndroid: boolean = /android/i.test(navigator.userAgent);

/** Default transfer progress state. */
export function defaultProgress(status = 'Initializing...'): TransferProgress {
  return {
    status,
    bytes_transferred: 0,
    total_bytes: 0,
    percent: 0,
  };
}
