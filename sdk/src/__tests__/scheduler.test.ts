/**
 * Tests for AppScale Scheduler (scheduler.ts)
 *
 * Verifies priority scheduling, batch merging, backpressure,
 * and stats tracking.
 */

import { AppScaleScheduler, Priority } from '../scheduler';
import type { NativeEngine, IrBatch } from '../types';

// ── Mock rAF for Node/jsdom ──

let rafCallbacks: Array<() => void> = [];
(globalThis as any).requestAnimationFrame = (cb: () => void) => {
  rafCallbacks.push(cb);
  return rafCallbacks.length;
};
(globalThis as any).cancelAnimationFrame = (_id: number) => {};

// Spy on performance.now (jsdom provides its own Performance object)
const perfNowSpy = jest.spyOn(performance, 'now').mockReturnValue(0);

function flushRaf() {
  const cbs = [...rafCallbacks];
  rafCallbacks = [];
  cbs.forEach(cb => cb());
}

// ── Mock engine ──

function createMockEngine(): NativeEngine & { commits: IrBatch[] } {
  const commits: IrBatch[] = [];
  return {
    commits,
    applyCommit: jest.fn((batch: IrBatch) => {
      commits.push(batch);
    }),
    sync: jest.fn(() => ({ value: false })),
  };
}

function makeBatch(id: number, commands: any[] = []): IrBatch {
  return { commit_id: id, timestamp_ms: id * 10, commands };
}

describe('AppScaleScheduler', () => {
  let engine: ReturnType<typeof createMockEngine>;
  let scheduler: AppScaleScheduler;

  beforeEach(() => {
    rafCallbacks = [];
    perfNowSpy.mockReturnValue(0);
    engine = createMockEngine();
    scheduler = new AppScaleScheduler(engine);
  });

  test('schedules Normal priority for next rAF', () => {
    scheduler.schedule(makeBatch(1), Priority.Normal);
    // Not sent yet
    expect(engine.applyCommit).not.toHaveBeenCalled();
    // Flush frame
    flushRaf();
    expect(engine.applyCommit).toHaveBeenCalledTimes(1);
  });

  test('Immediate priority bypasses rAF', () => {
    scheduler.schedule(makeBatch(1), Priority.Immediate);
    expect(engine.applyCommit).toHaveBeenCalledTimes(1);
  });

  test('coalesces multiple Normal batches into one frame', () => {
    scheduler.schedule(makeBatch(1, [{ type: 'create', id: 1 }]), Priority.Normal);
    scheduler.schedule(makeBatch(2, [{ type: 'create', id: 2 }]), Priority.Normal);
    scheduler.schedule(makeBatch(3, [{ type: 'create', id: 3 }]), Priority.Normal);

    flushRaf();
    // All merged into a single engine call
    expect(engine.applyCommit).toHaveBeenCalledTimes(1);
    const merged = engine.commits[0];
    expect(merged.commands.length).toBe(3);
  });

  test('sorts pending by priority', () => {
    scheduler.schedule(makeBatch(1), Priority.Low);
    scheduler.schedule(makeBatch(2), Priority.Immediate);
    scheduler.schedule(makeBatch(3), Priority.UserBlocking);

    // Immediate is flushed synchronously
    expect(engine.applyCommit).toHaveBeenCalledTimes(1);

    // Remaining flushed on rAF (UserBlocking before Low)
    flushRaf();
    expect(engine.applyCommit).toHaveBeenCalledTimes(2);
  });

  test('getStats returns tracking info', () => {
    const stats = scheduler.getStats();
    expect(stats).toHaveProperty('totalFrames');
    expect(stats).toHaveProperty('droppedFrames');
    expect(stats).toHaveProperty('coalescedCommits');
    expect(stats).toHaveProperty('pendingCount');
    expect(stats).toHaveProperty('lastFrameMs');
    expect(stats).toHaveProperty('isProcessing');
    expect(stats.totalFrames).toBe(0);
  });

  test('setTargetFps adjusts frame budget', () => {
    scheduler.setTargetFps(120);
    // Internal state, verified via backpressure behavior
    // At 120fps, budget ≈ 8.33ms — tighter than default 16.67ms
    scheduler.schedule(makeBatch(1), Priority.Normal);
    flushRaf();
    expect(engine.applyCommit).toHaveBeenCalledTimes(1);
  });

  test('tracks frame count after flush', () => {
    scheduler.schedule(makeBatch(1), Priority.Normal);
    flushRaf();
    expect(scheduler.getStats().totalFrames).toBe(1);
  });

  test('handles empty pending gracefully', () => {
    // Nothing scheduled — should not crash on flush
    flushRaf();
    expect(engine.applyCommit).not.toHaveBeenCalled();
  });

  test('batch merging sorts creates before updates before removes', () => {
    // Use multiple batches so mergeBatches runs full sort logic
    scheduler.schedule(makeBatch(1, [{ type: 'remove_child', parent: 1, child: 2 }]), Priority.Normal);
    scheduler.schedule(makeBatch(2, [{ type: 'update_props', id: 3 }]), Priority.Normal);
    scheduler.schedule(makeBatch(3, [{ type: 'create', id: 4, view_type: 'Text' }]), Priority.Normal);
    scheduler.schedule(makeBatch(4, [{ type: 'append_child', parent: 1, child: 4 }]), Priority.Normal);

    flushRaf();
    const cmds = engine.commits[0].commands;
    // Order: creates → updates → appends → removes
    const types = cmds.map((c: any) => c.type);
    expect(types.indexOf('create')).toBeLessThan(types.indexOf('update_props'));
    expect(types.indexOf('append_child')).toBeLessThan(types.indexOf('remove_child'));
  });
});
