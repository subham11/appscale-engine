/**
 * AppScale Bridge — Type-safe JS ↔ Rust communication layer.
 *
 * This module provides the full hybrid sync/async bridge contract.
 * Sync calls use `&self` on the Rust side (compile-time mutation safety).
 * Async calls enqueue mutations for the next frame via the scheduler.
 *
 * Architecture:
 *   React Component → bridge.ts → JSI/FFI → Rust Engine
 *   React Component ← NativeCallback ← Rust Engine (events)
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core types (match Rust bridge.rs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export type NodeId = number;

export interface TextStyleInput {
  fontSize?: number;
  fontFamily?: string;
  fontWeight?: string;
}

export interface LayoutResult {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface TextMetrics {
  width: number;
  height: number;
  baseline: number;
  lineCount: number;
}

export interface ScreenInfo {
  width: number;
  height: number;
  scale: number;
}

export interface ScrollOffset {
  x: number;
  y: number;
}

export interface ActiveRoute {
  routeName: string | null;
  params: Record<string, string>;
}

export interface FrameStats {
  frameCount: number;
  framesDropped: number;
  lastFrameMs: number;
  lastLayoutMs: number;
  lastMountMs: number;
}

export interface IrCommand {
  type: string;
  [key: string]: unknown;
}

export interface IrBatch {
  commit_id: number;
  timestamp_ms: number;
  commands: IrCommand[];
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Native engine interface (platform-provided via JSI)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface NativeEngine {
  /** Synchronous read-only call → Rust. Returns JSON result. */
  syncCall(json: string): string;
  /** Asynchronous mutation call → Rust. Returns immediately. */
  asyncCall(json: string): void;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Bridge singleton
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

let _engine: NativeEngine | null = null;

/** Initialize the bridge with the platform-provided native engine. */
export function initBridge(engine: NativeEngine): void {
  _engine = engine;
}

/** @internal Reset bridge state — test use only. */
export function _resetBridge(): void {
  _engine = null;
}

function getEngine(): NativeEngine {
  if (!_engine) {
    throw new Error('AppScale bridge not initialized. Call initBridge() first.');
  }
  return _engine;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Sync methods — read-only, immediate return
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function syncCall<T>(payload: Record<string, unknown>): T {
  const engine = getEngine();
  const result = engine.syncCall(JSON.stringify(payload));
  return JSON.parse(result) as T;
}

/** Measure a node's computed layout (x, y, width, height). */
export function measure(nodeId: NodeId): LayoutResult | null {
  const r = syncCall<{ result: string; x?: number; y?: number; width?: number; height?: number }>({
    call: 'measure',
    node_id: nodeId,
  });
  if (r.result === 'not_found') return null;
  return { x: r.x!, y: r.y!, width: r.width!, height: r.height! };
}

/** Measure text dimensions without creating a node. */
export function measureText(
  text: string,
  style?: TextStyleInput,
  maxWidth?: number,
): TextMetrics {
  const r = syncCall<{ width: number; height: number; baseline: number; line_count: number }>({
    call: 'measure_text',
    text,
    style: style
      ? { font_size: style.fontSize, font_family: style.fontFamily, font_weight: style.fontWeight }
      : {},
    max_width: maxWidth ?? Infinity,
  });
  return { width: r.width, height: r.height, baseline: r.baseline, lineCount: r.line_count };
}

/** Check if a node currently has focus. */
export function isFocused(nodeId: NodeId): boolean {
  const r = syncCall<{ value: boolean }>({ call: 'is_focused', node_id: nodeId });
  return r.value;
}

/** Get the node that currently holds focus (if any). */
export function getFocusedNode(): NodeId | null {
  const r = syncCall<{ node_id: number | null }>({ call: 'get_focused_node' });
  return r.node_id;
}

/** Get a node's current scroll offset. */
export function getScrollOffset(nodeId: NodeId): ScrollOffset {
  const r = syncCall<{ x: number; y: number }>({ call: 'get_scroll_offset', node_id: nodeId });
  return r;
}

/** Check if the platform supports a capability. */
export function supports(capability: string): boolean {
  const r = syncCall<{ value: boolean }>({ call: 'supports_capability', capability });
  return r.value;
}

/** Get screen dimensions and scale factor. */
export function getScreenSize(): ScreenInfo {
  return syncCall<ScreenInfo>({ call: 'get_screen_info' });
}

/** Check if the Rust scheduler is currently processing. */
export function isProcessing(): boolean {
  const r = syncCall<{ value: boolean }>({ call: 'is_processing' });
  return r.value;
}

/** Check if a node exists in the shadow tree. */
export function nodeExists(nodeId: NodeId): boolean {
  const r = syncCall<{ value: boolean }>({ call: 'node_exists', node_id: nodeId });
  return r.value;
}

/** Get the child count of a node. */
export function getChildCount(nodeId: NodeId): number {
  const r = syncCall<{ value: number }>({ call: 'get_child_count', node_id: nodeId });
  return r.value;
}

/** Check if back navigation is possible. */
export function canGoBack(): boolean {
  const r = syncCall<{ value: boolean }>({ call: 'can_go_back' });
  return r.value;
}

/** Get the currently active route. */
export function getActiveRoute(): ActiveRoute {
  const r = syncCall<{ route_name: string | null; params: Record<string, string> }>({
    call: 'get_active_route',
  });
  return { routeName: r.route_name, params: r.params };
}

/** Get scheduler frame stats (DevTools). */
export function getFrameStats(): FrameStats {
  const r = syncCall<{
    frame_count: number;
    frames_dropped: number;
    last_frame_ms: number;
    last_layout_ms: number;
    last_mount_ms: number;
  }>({ call: 'get_frame_stats' });
  return {
    frameCount: r.frame_count,
    framesDropped: r.frames_dropped,
    lastFrameMs: r.last_frame_ms,
    lastLayoutMs: r.last_layout_ms,
    lastMountMs: r.last_mount_ms,
  };
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Async methods — mutations, enqueued for next frame
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function asyncCall(payload: Record<string, unknown>): void {
  const engine = getEngine();
  engine.asyncCall(JSON.stringify(payload));
}

/** Apply a batch of IR commands (all UI mutations go through here). */
export function applyCommit(batch: IrBatch): void {
  asyncCall({ call: 'apply_commit', batch });
}

/** Navigate: push, pop, replace, modal, deep link, tab switch. */
export function navigate(
  action: string,
  options?: {
    route?: string;
    params?: Record<string, string>;
    url?: string;
    index?: number;
  },
): void {
  asyncCall({
    call: 'navigate',
    action,
    route: options?.route,
    params: options?.params ?? {},
    url: options?.url,
    index: options?.index,
  });
}

/** Set focus to a specific node. */
export function setFocus(nodeId: NodeId): void {
  asyncCall({ call: 'set_focus', node_id: nodeId });
}

/** Move focus in a direction ("next" | "previous"). */
export function moveFocus(direction: 'next' | 'previous'): void {
  asyncCall({ call: 'move_focus', direction });
}

/** Announce a message via screen reader (VoiceOver / TalkBack). */
export function announce(message: string): void {
  asyncCall({ call: 'announce', message });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// React hooks
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/** Hook: measure a node's layout. Re-measures when deps change. */
export function useLayout(nodeId: NodeId | null): LayoutResult | null {
  const [layout, setLayout] = useState<LayoutResult | null>(null);

  useEffect(() => {
    if (nodeId == null) {
      setLayout(null);
      return;
    }
    // Measure after the current frame completes
    const raf = requestAnimationFrame(() => {
      setLayout(measure(nodeId));
    });
    return () => cancelAnimationFrame(raf);
  }, [nodeId]);

  return layout;
}

/** Hook: query a platform capability. Memoized. */
export function usePlatformCapability(capability: string): boolean {
  return useMemo(() => supports(capability), [capability]);
}

/** Hook: get current screen size. Updates on resize. */
export function useScreenSize(): ScreenInfo {
  const [size, setSize] = useState<ScreenInfo>(() => getScreenSize());

  useEffect(() => {
    // Poll on animation frame — native resize events will
    // come through NativeCallback, but this is a fallback.
    let active = true;
    const poll = () => {
      if (!active) return;
      const next = getScreenSize();
      setSize((prev) => {
        if (prev.width !== next.width || prev.height !== next.height || prev.scale !== next.scale) {
          return next;
        }
        return prev;
      });
    };
    // Check once after mount
    const raf = requestAnimationFrame(poll);
    return () => {
      active = false;
      cancelAnimationFrame(raf);
    };
  }, []);

  return size;
}

/** Hook: navigation state (active route + canGoBack). */
export function useNavigation() {
  const [route, setRoute] = useState<ActiveRoute>(() => getActiveRoute());
  const [back, setBack] = useState<boolean>(() => canGoBack());
  const pollRef = useRef<number>(0);

  useEffect(() => {
    let active = true;
    const poll = () => {
      if (!active) return;
      setRoute(getActiveRoute());
      setBack(canGoBack());
      pollRef.current = requestAnimationFrame(poll);
    };
    pollRef.current = requestAnimationFrame(poll);
    return () => {
      active = false;
      if (pollRef.current != null) cancelAnimationFrame(pollRef.current);
    };
  }, []);

  const push = useCallback(
    (routeName: string, params?: Record<string, string>) =>
      navigate('push', { route: routeName, params }),
    [],
  );
  const pop = useCallback(() => navigate('pop'), []);
  const replace = useCallback(
    (routeName: string, params?: Record<string, string>) =>
      navigate('replace', { route: routeName, params }),
    [],
  );
  const goBack = useCallback(() => navigate('goBack'), []);

  return { route, canGoBack: back, push, pop, replace, goBack };
}
