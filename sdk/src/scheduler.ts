/**
 * AppScale Scheduler — JS-side frame coordination.
 *
 * Coordinates React's asynchronous update batching with the Rust engine's
 * synchronous frame processing. Uses the hybrid bridge model:
 *
 *   ASYNC: applyCommit(batch) — all UI mutations, frame-batched
 *   SYNC:  sync('is_processing') — backpressure query, immediate return
 *
 * Handles:
 * - Frame coalescing (multiple React commits → one Rust frame)
 * - Backpressure (slow Rust → skip/coalesce JS commits, queried via sync path)
 * - Priority lanes (input > interaction > data > idle)
 * - Alignment with requestAnimationFrame for smooth rendering
 */

import type { IrBatch, NativeEngine } from './types';

export enum Priority {
  Immediate = 0,      // User input (touch feedback)
  UserBlocking = 1,   // Discrete interactions (button press)
  Normal = 2,         // State updates (data fetch)
  Low = 3,            // Background work
  Idle = 4,           // Cleanup
}

interface PendingCommit {
  batch: IrBatch;
  priority: Priority;
  timestamp: number;
}

/**
 * The scheduler sits between the React reconciler and the Rust engine.
 * It never sends more than one batch per animation frame unless priority is Immediate.
 */
export class AppScaleScheduler {
  private engine: NativeEngine;
  private pending: PendingCommit[] = [];
  private frameScheduled = false;
  private processing = false;

  // Backpressure tracking
  private lastFrameDuration = 0;
  private frameBudgetMs = 16.67;

  // Stats for DevTools
  private totalFrames = 0;
  private droppedFrames = 0;
  private coalescedCommits = 0;

  constructor(engine: NativeEngine) {
    this.engine = engine;
  }

  /**
   * Schedule a commit for processing.
   * Called by the host config's resetAfterCommit().
   */
  schedule(batch: IrBatch, priority: Priority = Priority.Normal): void {
    this.pending.push({
      batch,
      priority,
      timestamp: performance.now(),
    });

    // Sort by priority (highest = lowest number = front)
    this.pending.sort((a, b) => a.priority - b.priority);

    if (priority === Priority.Immediate) {
      // Immediate: bypass frame scheduling, process synchronously
      this.flushImmediate();
    } else if (!this.frameScheduled) {
      this.frameScheduled = true;
      requestAnimationFrame(() => this.flushFrame());
    }
  }

  /**
   * Process Immediate-priority commits synchronously.
   * Used for touch feedback — can't wait for next rAF.
   */
  private flushImmediate(): void {
    const immediate = this.pending.filter(p => p.priority === Priority.Immediate);
    this.pending = this.pending.filter(p => p.priority !== Priority.Immediate);

    for (const commit of immediate) {
      this.sendToEngine(commit.batch);
    }
  }

  /**
   * Process pending commits aligned with animation frame.
   * Coalesces multiple Normal/Low commits into a single engine call.
   */
  private flushFrame(): void {
    this.frameScheduled = false;
    this.totalFrames++;

    if (this.pending.length === 0) return;

    // Backpressure: if last frame took too long, only process high-priority
    const underPressure = this.lastFrameDuration > this.frameBudgetMs;

    if (underPressure) {
      // Only process UserBlocking and above
      const urgent = this.pending.filter(p => p.priority <= Priority.UserBlocking);
      const deferred = this.pending.filter(p => p.priority > Priority.UserBlocking);

      this.pending = deferred;
      this.coalescedCommits += deferred.length;

      if (urgent.length > 0) {
        // Merge all urgent batches into one
        const merged = this.mergeBatches(urgent.map(u => u.batch));
        this.sendToEngine(merged);
      }

      // Reschedule deferred work
      if (this.pending.length > 0 && !this.frameScheduled) {
        this.frameScheduled = true;
        requestAnimationFrame(() => this.flushFrame());
      }
    } else {
      // Normal: process all pending (coalesce into one engine call)
      const batches = this.pending.map(p => p.batch);
      this.pending = [];

      const merged = this.mergeBatches(batches);
      this.sendToEngine(merged);
    }
  }

  /**
   * Merge multiple IR batches into one (command concatenation).
   * This is safe because IR commands are order-independent within a commit
   * (creates before updates before deletes).
   */
  private mergeBatches(batches: IrBatch[]): IrBatch {
    if (batches.length === 1) return batches[0];

    const merged: IrBatch = {
      commit_id: batches[batches.length - 1].commit_id,
      timestamp_ms: performance.now(),
      commands: [],
    };

    // Order: all creates → all updates → all appends → all removes
    const creates: any[] = [];
    const updates: any[] = [];
    const appends: any[] = [];
    const removes: any[] = [];

    for (const batch of batches) {
      for (const cmd of batch.commands) {
        switch (cmd.type) {
          case 'create': creates.push(cmd); break;
          case 'update_props':
          case 'update_style': updates.push(cmd); break;
          case 'append_child':
          case 'insert_before':
          case 'set_root': appends.push(cmd); break;
          case 'remove_child': removes.push(cmd); break;
          default: updates.push(cmd);
        }
      }
    }

    // Deduplicate updates: keep only the latest update per node ID
    const latestUpdates = new Map<number, any>();
    for (const cmd of updates) {
      latestUpdates.set(cmd.id, cmd);
    }

    merged.commands = [
      ...creates,
      ...latestUpdates.values(),
      ...appends,
      ...removes,
    ];

    return merged;
  }

  /**
   * Send a batch to the Rust engine and track timing.
   */
  private sendToEngine(batch: IrBatch): void {
    this.processing = true;
    const start = performance.now();

    try {
      this.engine.applyCommit(batch);
    } catch (error) {
      console.error('[AppScale Scheduler] Engine error:', error);
    } finally {
      this.lastFrameDuration = performance.now() - start;
      this.processing = false;

      if (this.lastFrameDuration > this.frameBudgetMs) {
        this.droppedFrames++;
      }
    }
  }

  /**
   * Get scheduler stats (for DevTools).
   */
  getStats() {
    return {
      totalFrames: this.totalFrames,
      droppedFrames: this.droppedFrames,
      coalescedCommits: this.coalescedCommits,
      pendingCount: this.pending.length,
      lastFrameMs: Math.round(this.lastFrameDuration * 100) / 100,
      isProcessing: this.processing,
    };
  }

  /**
   * Set frame budget for the target refresh rate.
   */
  setTargetFps(fps: number): void {
    this.frameBudgetMs = 1000 / fps;
  }
}
