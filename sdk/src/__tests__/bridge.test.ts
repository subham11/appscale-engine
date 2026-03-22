/**
 * Tests for AppScale Bridge (bridge.ts)
 *
 * Verifies sync/async call dispatch, bridge initialization,
 * and all typed wrapper functions.
 */

jest.mock('react', () => ({
  useCallback: jest.fn((fn: any) => fn),
  useEffect: jest.fn(),
  useMemo: jest.fn((fn: any) => fn()),
  useRef: jest.fn(() => ({ current: null })),
  useState: jest.fn((init: any) => [init, jest.fn()]),
}));

import {
  initBridge,
  _resetBridge,
  measure,
  isFocused,
  getFocusedNode,
  getScrollOffset,
  supports,
  getScreenSize,
  isProcessing,
  nodeExists,
  getChildCount,
  canGoBack,
  getActiveRoute,
  getFrameStats,
  applyCommit,
  navigate,
  setFocus,
  announce,
} from '../bridge';
import type { NativeEngine } from '../bridge';

// ── Mock engine ──

function createMockEngine(syncResponses: Record<string, unknown> = {}) {
  return {
    syncCall: jest.fn((json: string) => {
      const parsed = JSON.parse(json);
      const key = parsed.call as string;
      const response = syncResponses[key] ?? {};
      return JSON.stringify(response);
    }),
    asyncCall: jest.fn(),
  };
}

beforeEach(() => {
  _resetBridge();
});

describe('Bridge initialization', () => {
  test('throws when bridge not initialized', () => {
    expect(() => measure(1)).toThrow('bridge not initialized');
  });

  test('works after initBridge()', () => {
    const engine = createMockEngine({
      measure: { result: 'ok', x: 0, y: 0, width: 100, height: 50 },
    });
    initBridge(engine as unknown as NativeEngine);
    const result = measure(1);
    expect(result).toEqual({ x: 0, y: 0, width: 100, height: 50 });
  });
});

describe('Sync bridge calls', () => {
  let engine: ReturnType<typeof createMockEngine>;

  function init(syncResponses: Record<string, unknown>) {
    engine = createMockEngine(syncResponses);
    initBridge(engine as unknown as NativeEngine);
  }

  test('measure returns layout result', () => {
    init({ measure: { result: 'ok', x: 10, y: 20, width: 300, height: 400 } });
    const result = measure(42);
    expect(result).toEqual({ x: 10, y: 20, width: 300, height: 400 });
    expect(engine.syncCall).toHaveBeenCalledWith(
      JSON.stringify({ call: 'measure', node_id: 42 }),
    );
  });

  test('measure returns null for not_found', () => {
    init({ measure: { result: 'not_found' } });
    expect(measure(999)).toBeNull();
  });

  test('isFocused returns boolean', () => {
    init({ is_focused: { value: true } });
    expect(isFocused(1)).toBe(true);
  });

  test('getFocusedNode returns node id or null', () => {
    init({ get_focused_node: { node_id: 5 } });
    expect(getFocusedNode()).toBe(5);
  });

  test('getScrollOffset returns x/y', () => {
    init({ get_scroll_offset: { x: 0, y: 150 } });
    expect(getScrollOffset(10)).toEqual({ x: 0, y: 150 });
  });

  test('supports returns capability check', () => {
    init({ supports_capability: { value: true } });
    expect(supports('haptics')).toBe(true);
  });

  test('getScreenSize returns screen info', () => {
    init({ get_screen_info: { width: 390, height: 844, scale: 3 } });
    expect(getScreenSize()).toEqual({ width: 390, height: 844, scale: 3 });
  });

  test('isProcessing returns boolean', () => {
    init({ is_processing: { value: false } });
    expect(isProcessing()).toBe(false);
  });

  test('nodeExists returns boolean', () => {
    init({ node_exists: { value: true } });
    expect(nodeExists(7)).toBe(true);
  });

  test('getChildCount returns count', () => {
    init({ get_child_count: { value: 3 } });
    expect(getChildCount(1)).toBe(3);
  });

  test('canGoBack returns boolean', () => {
    init({ can_go_back: { value: false } });
    expect(canGoBack()).toBe(false);
  });

  test('getActiveRoute returns route info', () => {
    init({ get_active_route: { route_name: 'Home', params: { id: '42' } } });
    expect(getActiveRoute()).toEqual({
      routeName: 'Home',
      params: { id: '42' },
    });
  });

  test('getFrameStats returns stats object', () => {
    init({
      get_frame_stats: {
        frame_count: 100,
        frames_dropped: 2,
        last_frame_ms: 8.5,
        last_layout_ms: 3.1,
        last_mount_ms: 2.0,
      },
    });
    expect(getFrameStats()).toEqual({
      frameCount: 100,
      framesDropped: 2,
      lastFrameMs: 8.5,
      lastLayoutMs: 3.1,
      lastMountMs: 2.0,
    });
  });
});

describe('Async bridge calls', () => {
  let engine: ReturnType<typeof createMockEngine>;

  beforeEach(() => {
    engine = createMockEngine();
    initBridge(engine as unknown as NativeEngine);
  });

  test('applyCommit sends batch', () => {
    const batch = { commit_id: 1, timestamp_ms: 100, commands: [] };
    applyCommit(batch);
    expect(engine.asyncCall).toHaveBeenCalledTimes(1);
    const payload = JSON.parse(engine.asyncCall.mock.calls[0][0]);
    expect(payload.call).toBe('apply_commit');
    expect(payload.batch).toEqual(batch);
  });

  test('navigate sends action with options', () => {
    navigate('push', { route: 'Details', params: { id: '1' } });
    const payload = JSON.parse(engine.asyncCall.mock.calls[0][0]);
    expect(payload.call).toBe('navigate');
    expect(payload.action).toBe('push');
    expect(payload.route).toBe('Details');
  });

  test('setFocus sends node id', () => {
    setFocus(42);
    const payload = JSON.parse(engine.asyncCall.mock.calls[0][0]);
    expect(payload.call).toBe('set_focus');
    expect(payload.node_id).toBe(42);
  });

  test('announce sends message', () => {
    announce('New content loaded');
    const payload = JSON.parse(engine.asyncCall.mock.calls[0][0]);
    expect(payload.call).toBe('announce');
    expect(payload.message).toBe('New content loaded');
  });
});
