/**
 * AppScale React Host Config
 *
 * This implements the react-reconciler host configuration.
 * It is the bridge between React's fiber tree and the Rust engine.
 *
 * Data flow:
 *   React Fiber → Reconciler (diff) → Commit Phase → Binary IR → Rust Core
 *
 * Key design decisions:
 * - We batch all mutations during the commit phase into an IrBatch
 * - The IrBatch is serialized and sent to Rust core in one shot
 * - No individual create/update calls during render — only during commit
 * - This matches React 18+ concurrent mode (render is interruptible, commit is not)
 */

import Reconciler from 'react-reconciler';
import {
  DefaultEventPriority,
  // ConcurrentRoot,  // Enable when ready for concurrent mode
} from 'react-reconciler/constants';
import { encodeBatch } from './ir-binary';

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types matching Rust IR layer
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface IrCommand {
  type: string;
  [key: string]: any;
}

interface IrBatch {
  commit_id: number;
  timestamp_ms: number;
  commands: IrCommand[];
}

interface NativeEngine {
  /**
   * ASYNC PATH — Send a batch of IR commands to the Rust core.
   * All UI mutations go through this path, batched per frame.
   * Implemented via FlatBuffers (Phase 2) or JSON postMessage (Phase 1).
   *
   * ❌ This is the ONLY way to mutate the UI tree.
   */
  applyCommit(batch: IrBatch): void;

  /**
   * BINARY PATH — Send a FlatBuffers-encoded batch directly.
   * If provided, this is used instead of applyCommit for zero-copy transport.
   * The binary format matches the ir-schema/ir.fbs schema.
   */
  applyCommitBinary?(bytes: Uint8Array): void;

  /**
   * SYNC PATH — Synchronous read-only call to Rust core.
   * Returns immediately with the result. No UI mutation allowed.
   * Implemented via JSI (production) or synchronous WASM call (web).
   *
   * Supported calls:
   *   measure(nodeId)         → { x, y, width, height }
   *   is_focused(nodeId)      → boolean
   *   get_scroll_offset(nodeId) → { x, y }
   *   supports_capability(cap) → boolean
   *   get_screen_info()       → { width, height, scale }
   *   is_processing()         → boolean
   *   node_exists(nodeId)     → boolean
   *   get_child_count(nodeId) → number
   */
  sync(call: string, ...args: any[]): any;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Node ID generation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

let nextNodeId = 1;
function allocNodeId(): number {
  return nextNodeId++;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Instance types (what React tracks per fiber node)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface AppScaleInstance {
  nodeId: number;
  viewType: string;
  props: Record<string, any>;
  style: Record<string, any>;
  children: AppScaleInstance[];
}

interface AppScaleTextInstance {
  nodeId: number;
  text: string;
}

type Container = { rootNodeId: number };
type HostContext = {};
type UpdatePayload = { propsDiff: Record<string, any>; styleDiff: Record<string, any> | null };

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Style extraction
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const LAYOUT_PROPS = new Set([
  'display', 'position', 'flexDirection', 'flexWrap',
  'flexGrow', 'flexShrink', 'justifyContent', 'alignItems',
  'width', 'height', 'minWidth', 'minHeight', 'maxWidth', 'maxHeight',
  'aspectRatio', 'margin', 'marginTop', 'marginRight', 'marginBottom', 'marginLeft',
  'padding', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
  'gap', 'overflow',
]);

function separateStyleAndProps(rawProps: Record<string, any>): {
  style: Record<string, any>;
  props: Record<string, any>;
} {
  const style: Record<string, any> = {};
  const props: Record<string, any> = {};

  for (const [key, value] of Object.entries(rawProps)) {
    if (key === 'style' && typeof value === 'object') {
      // Inline style object — split into layout and visual props
      for (const [sk, sv] of Object.entries(value)) {
        if (LAYOUT_PROPS.has(sk)) {
          style[sk] = sv;
        } else {
          props[sk] = sv;  // backgroundColor, color, borderRadius, etc.
        }
      }
    } else if (key === 'children' || key === 'ref' || key === 'key') {
      // React internal props — skip
    } else if (typeof value === 'function') {
      // Event handlers — tracked separately, not sent via IR
      // (registered via the event system)
    } else {
      props[key] = value;
    }
  }

  return { style, props };
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Commit batch accumulator
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

let commitId = 0;
let pendingBatch: IrCommand[] = [];

function pushCommand(cmd: IrCommand) {
  pendingBatch.push(cmd);
}

function flushBatch(engine: NativeEngine) {
  if (pendingBatch.length === 0) return;

  commitId++;
  const batch: IrBatch = {
    commit_id: commitId,
    timestamp_ms: performance.now(),
    commands: pendingBatch,
  };

  pendingBatch = [];

  // Prefer binary transport (FlatBuffers) when the engine supports it.
  // Falls back to JSON for dev/web mode.
  if (engine.applyCommitBinary) {
    const bytes = encodeBatch(batch);
    engine.applyCommitBinary(bytes);
  } else {
    engine.applyCommit(batch);
  }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Host Config implementation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function createHostConfig(engine: NativeEngine) {
  const hostConfig: Reconciler.HostConfig<
    string,                    // Type (e.g., 'View', 'Text', 'TextInput')
    Record<string, any>,       // Props
    Container,                 // Container
    AppScaleInstance,          // Instance
    AppScaleTextInstance,      // TextInstance
    never,                     // SuspenseInstance
    never,                     // HydratableInstance
    AppScaleInstance,          // PublicInstance
    HostContext,                // HostContext
    UpdatePayload,             // UpdatePayload
    never,                     // ChildSet (not used in mutation mode)
    number,                    // TimeoutHandle
    number                     // NoTimeout
  > = {
    // ━━━ Configuration ━━━
    supportsMutation: true,
    supportsPersistence: false,
    supportsHydration: false,
    isPrimaryRenderer: true,

    // ━━━ Instance creation ━━━

    createInstance(
      type: string,
      props: Record<string, any>,
      rootContainer: Container,
      hostContext: HostContext,
      internalHandle: any,
    ): AppScaleInstance {
      const nodeId = allocNodeId();
      const { style, props: viewProps } = separateStyleAndProps(props);

      pushCommand({
        type: 'create',
        id: nodeId,
        view_type: type,
        props: viewProps,
        style,
      });

      return {
        nodeId,
        viewType: type,
        props: viewProps,
        style,
        children: [],
      };
    },

    createTextInstance(
      text: string,
      rootContainer: Container,
      hostContext: HostContext,
      internalHandle: any,
    ): AppScaleTextInstance {
      const nodeId = allocNodeId();

      pushCommand({
        type: 'create',
        id: nodeId,
        view_type: 'Text',
        props: { text },
        style: {},
      });

      return { nodeId, text };
    },

    // ━━━ Tree mutations ━━━

    appendInitialChild(parent: AppScaleInstance, child: AppScaleInstance | AppScaleTextInstance) {
      parent.children.push(child as AppScaleInstance);
      pushCommand({
        type: 'append_child',
        parent: parent.nodeId,
        child: child.nodeId,
      });
    },

    appendChild(parent: AppScaleInstance, child: AppScaleInstance | AppScaleTextInstance) {
      parent.children.push(child as AppScaleInstance);
      pushCommand({
        type: 'append_child',
        parent: parent.nodeId,
        child: child.nodeId,
      });
    },

    appendChildToContainer(container: Container, child: AppScaleInstance | AppScaleTextInstance) {
      pushCommand({
        type: 'append_child',
        parent: container.rootNodeId,
        child: child.nodeId,
      });
    },

    insertBefore(
      parent: AppScaleInstance,
      child: AppScaleInstance | AppScaleTextInstance,
      beforeChild: AppScaleInstance | AppScaleTextInstance,
    ) {
      pushCommand({
        type: 'insert_before',
        parent: parent.nodeId,
        child: child.nodeId,
        before: beforeChild.nodeId,
      });
    },

    insertInContainerBefore(
      container: Container,
      child: AppScaleInstance | AppScaleTextInstance,
      beforeChild: AppScaleInstance | AppScaleTextInstance,
    ) {
      pushCommand({
        type: 'insert_before',
        parent: container.rootNodeId,
        child: child.nodeId,
        before: beforeChild.nodeId,
      });
    },

    removeChild(parent: AppScaleInstance, child: AppScaleInstance | AppScaleTextInstance) {
      parent.children = parent.children.filter(c => c.nodeId !== child.nodeId);
      pushCommand({
        type: 'remove_child',
        parent: parent.nodeId,
        child: child.nodeId,
      });
    },

    removeChildFromContainer(container: Container, child: AppScaleInstance | AppScaleTextInstance) {
      pushCommand({
        type: 'remove_child',
        parent: container.rootNodeId,
        child: child.nodeId,
      });
    },

    // ━━━ Updates ━━━

    prepareUpdate(
      instance: AppScaleInstance,
      type: string,
      oldProps: Record<string, any>,
      newProps: Record<string, any>,
    ): UpdatePayload | null {
      const { style: oldStyle, props: oldViewProps } = separateStyleAndProps(oldProps);
      const { style: newStyle, props: newViewProps } = separateStyleAndProps(newProps);

      // Diff props
      const propsDiff: Record<string, any> = {};
      let propsChanged = false;

      for (const key of new Set([...Object.keys(oldViewProps), ...Object.keys(newViewProps)])) {
        if (oldViewProps[key] !== newViewProps[key]) {
          propsDiff[key] = newViewProps[key] ?? null;
          propsChanged = true;
        }
      }

      // Diff style
      const styleDiff: Record<string, any> = {};
      let styleChanged = false;

      for (const key of new Set([...Object.keys(oldStyle), ...Object.keys(newStyle)])) {
        if (oldStyle[key] !== newStyle[key]) {
          styleDiff[key] = newStyle[key] ?? null;
          styleChanged = true;
        }
      }

      if (!propsChanged && !styleChanged) return null;

      return {
        propsDiff: propsChanged ? propsDiff : {},
        styleDiff: styleChanged ? styleDiff : null,
      };
    },

    commitUpdate(
      instance: AppScaleInstance,
      updatePayload: UpdatePayload,
      type: string,
      prevProps: Record<string, any>,
      nextProps: Record<string, any>,
    ) {
      if (Object.keys(updatePayload.propsDiff).length > 0) {
        pushCommand({
          type: 'update_props',
          id: instance.nodeId,
          diff: { changes: updatePayload.propsDiff },
        });
        Object.assign(instance.props, updatePayload.propsDiff);
      }

      if (updatePayload.styleDiff) {
        pushCommand({
          type: 'update_style',
          id: instance.nodeId,
          style: { ...instance.style, ...updatePayload.styleDiff },
        });
        Object.assign(instance.style, updatePayload.styleDiff);
      }
    },

    commitTextUpdate(textInstance: AppScaleTextInstance, oldText: string, newText: string) {
      if (oldText !== newText) {
        textInstance.text = newText;
        pushCommand({
          type: 'update_props',
          id: textInstance.nodeId,
          diff: { changes: { text: newText } },
        });
      }
    },

    // ━━━ Commit lifecycle ━━━

    prepareForCommit() {
      // Reset the command batch
      pendingBatch = [];
      return null;
    },

    resetAfterCommit() {
      // Flush all accumulated commands to Rust as a single batch
      flushBatch(engine);
    },

    // ━━━ Context ━━━

    getRootHostContext(): HostContext { return {}; },
    getChildHostContext(parentContext: HostContext): HostContext { return parentContext; },

    // ━━━ Misc ━━━

    getPublicInstance(instance: AppScaleInstance) { return instance; },
    finalizeInitialChildren() { return false; },
    shouldSetTextContent() { return false; },
    clearContainer() {},

    // ━━━ Scheduling ━━━

    getCurrentEventPriority() { return DefaultEventPriority; },
    getInstanceFromNode() { return null; },
    beforeActiveInstanceBlur() {},
    afterActiveInstanceBlur() {},
    prepareScopeUpdate() {},
    getInstanceFromScope() { return null; },
    detachDeletedInstance() {},
    preparePortalMount() {},

    scheduleTimeout: setTimeout,
    cancelTimeout: clearTimeout,
    noTimeout: -1,

    supportsMicrotasks: true,
    scheduleMicrotask: queueMicrotask,
  };

  return hostConfig;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Public API
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function createAppScaleRenderer(engine: NativeEngine) {
  const hostConfig = createHostConfig(engine);
  const reconciler = Reconciler(hostConfig);

  // ━━━ Sync bridge (read-only queries) ━━━
  const sync = {
    /** Measure a node's computed layout. Returns { x, y, width, height } or null. */
    measure(nodeId: number): { x: number; y: number; width: number; height: number } | null {
      const result = engine.sync('measure', nodeId);
      return result?.result === 'not_found' ? null : result;
    },

    /** Check if a node currently has focus. */
    isFocused(nodeId: number): boolean {
      return engine.sync('is_focused', nodeId)?.value ?? false;
    },

    /** Get scroll offset of a ScrollView node. */
    getScrollOffset(nodeId: number): { x: number; y: number } {
      return engine.sync('get_scroll_offset', nodeId) ?? { x: 0, y: 0 };
    },

    /** Query a platform capability. */
    supportsCapability(cap: string): boolean {
      return engine.sync('supports_capability', cap)?.value ?? false;
    },

    /** Get screen dimensions and scale factor. */
    getScreenInfo(): { width: number; height: number; scale: number } {
      return engine.sync('get_screen_info');
    },

    /** Check if engine is processing (backpressure signal for scheduler). */
    isProcessing(): boolean {
      return engine.sync('is_processing')?.value ?? false;
    },

    /** Check if a node exists in the shadow tree. */
    nodeExists(nodeId: number): boolean {
      return engine.sync('node_exists', nodeId)?.value ?? false;
    },

    /** Get child count of a node. */
    getChildCount(nodeId: number): number {
      return engine.sync('get_child_count', nodeId)?.value ?? 0;
    },

    /** Get frame stats (for DevTools). */
    getFrameStats(): {
      frame_count: number;
      frames_dropped: number;
      last_frame_ms: number;
      last_layout_ms: number;
      last_mount_ms: number;
    } {
      return engine.sync('get_frame_stats');
    },
  };

  return {
    render(element: React.ReactElement, callback?: () => void) {
      // Create root container node in the engine
      const rootNodeId = allocNodeId();
      pushCommand({
        type: 'create',
        id: rootNodeId,
        view_type: 'Container',
        props: {},
        style: { width: { Points: 390 }, height: { Points: 844 } },
      });
      pushCommand({ type: 'set_root', id: rootNodeId });
      flushBatch(engine);

      const container: Container = { rootNodeId };

      // Create React root
      const root = reconciler.createContainer(
        container,
        0,      // tag (LegacyRoot = 0, ConcurrentRoot = 1)
        null,   // hydrationCallbacks
        false,  // isStrictMode
        null,   // concurrentUpdatesByDefaultOverride
        '',     // identifierPrefix
        (error: Error) => console.error('[AppScale]', error),
        null,   // transitionCallbacks
      );

      // Schedule initial render
      reconciler.updateContainer(element, root, null, callback ?? (() => {}));

      return {
        unmount() {
          reconciler.updateContainer(null, root, null, () => {});
        },
      };
    },

    /**
     * Sync bridge — read-only queries to Rust core.
     * These return immediately via JSI (no async, no batching).
     *
     * RULE: No UI mutation through this interface.
     */
    sync,
  };
}

/**
 * Example usage:
 *
 * ```tsx
 * import { createAppScaleRenderer } from '@appscale/renderer';
 * import App from './App';
 *
 * // In production: engine is a JSI binding to Rust core
 * // In dev/web: engine serializes to JSON and posts to WASM
 * const engine = getNativeEngine();
 *
 * const renderer = createAppScaleRenderer(engine);
 * renderer.render(<App />);
 * ```
 */
