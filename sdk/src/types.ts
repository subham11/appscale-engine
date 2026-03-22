/**
 * Shared types for the AppScale TypeScript SDK.
 *
 * These types mirror the Rust IR layer and bridge contracts.
 */

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// IR types (matches Rust ir.rs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface IrCommand {
  type: string;
  [key: string]: any;
}

export interface IrBatch {
  commit_id: number;
  timestamp_ms: number;
  commands: IrCommand[];
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Engine interface (hybrid sync/async bridge)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface NativeEngine {
  /**
   * ASYNC PATH — Send a batch of IR commands to the Rust core.
   * All UI mutations go through this path, batched per frame.
   *
   * ❌ This is the ONLY way to mutate the UI tree.
   */
  applyCommit(batch: IrBatch): void;

  /**
   * SYNC PATH — Synchronous read-only call to Rust core.
   * Returns immediately with the result. No UI mutation allowed.
   *
   * Supported calls:
   *   "measure"              → { x, y, width, height }
   *   "is_focused"           → { value: boolean }
   *   "get_scroll_offset"    → { x, y }
   *   "supports_capability"  → { value: boolean }
   *   "get_screen_info"      → { width, height, scale }
   *   "is_processing"        → { value: boolean }
   *   "node_exists"          → { value: boolean }
   *   "get_child_count"      → { value: number }
   *   "get_frame_stats"      → { frame_count, frames_dropped, ... }
   */
  sync(call: string, ...args: any[]): any;
}
