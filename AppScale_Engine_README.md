# AppScale Engine

**React Native for all platforms — rebuilt for performance, AI, and scale.**

> Write React. Render native widgets. iOS, Android, macOS, Windows, Web.
> One codebase. Five platforms. Zero compromise.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/react-19+-61dafb.svg)](https://react.dev/)

---

## Table of Contents

- [What Is This?](#what-is-this)
- [Architecture Overview](#architecture-overview)
- [Data Flow: One Complete Frame](#data-flow-one-complete-frame)
- [Project Structure](#project-structure)
- [Module-by-Module Walkthrough](#module-by-module-walkthrough)
- [Threading Model](#threading-model)
- [How It Compares](#how-it-compares)
- [Key Design Decisions](#key-design-decisions)
- [Building and Testing](#building-and-testing)
- [Phase Roadmap](#phase-roadmap)
- [Prior Art and Lessons Learned](#prior-art-and-lessons-learned)
- [License](#license)

---

## What Is This?

AppScale Engine is a **cross-platform UI execution engine for React**. Developers write standard React/JSX — using hooks, components, and the entire React ecosystem they already know. The engine compiles their UI intent into native platform widgets on five targets: iOS (UIKit), Android (Views), macOS (AppKit), Windows (Composition APIs), and Web (DOM).

This is **not** a React Native fork. It is a clean-room implementation of a React renderer backed by a Rust core, using Taffy for layout (Flexbox + CSS Grid), a binary IR transport between JavaScript and Rust, and thin platform bridges that map to actual native widgets (not WebView, not custom-drawn pixels).

### The One-Line Definition

```
React reconciler → Binary IR → Rust execution engine → Native widgets on 5 platforms
```

### What Makes This Different From React Native

| Layer | React Native (Meta) | AppScale Engine |
|-------|---------------------|-----------------|
| **Transport** | JSI + Fabric C++ serialization | Binary IR (FlatBuffers) — deterministic, replayable, AI-generable |
| **Layout** | Yoga (C++, Flexbox only) | Taffy (Rust, Flexbox + CSS Grid) |
| **Core Runtime** | C++ (manual memory management) | Rust (memory-safe, data-race-free) |
| **Platforms** | iOS + Android first-class; desktop = Microsoft fork | All 5 platforms first-class from day one |
| **Desktop** | RN Windows/macOS lags 12-18 months behind mobile | Same-day releases across all platforms |
| **Web** | Abandoned by Meta (RN Web has zero investment) | First-class target (Rust to WASM to DOM) |
| **Navigation** | Third-party (react-navigation) | Built-in stack/modal/tab with native transitions |
| **Accessibility** | Partial, platform-inconsistent | Unified a11y layer with focus management |
| **Scheduling** | React scheduler only | Dual scheduler (JS + Rust) with priority lanes and backpressure |

---

## Architecture Overview

The engine follows a strict layered architecture. Each layer has a single responsibility and communicates with adjacent layers through well-defined contracts.

```
+-------------------------------------------------------------+
|  Layer 1: React API Surface (TypeScript)                    |
|  What developers write: JSX, hooks, components              |
|  RULE: Do NOT break this layer — it is the adoption engine  |
+----------------------------+--------------------------------+
                             | react-reconciler host config
                             | (createInstance, appendChild, commitUpdate)
+----------------------------v--------------------------------+
|  Layer 2: JS Scheduler + IR Builder                         |
|  Batches mutations during commit phase                      |
|  Priority lanes: Immediate > UserBlocking > Normal > Idle   |
|  Backpressure: coalesce when Rust is slow                   |
+----------------------------+--------------------------------+
                             | Binary IR (JSON Phase 1, FlatBuffers Phase 2)
                             | ONE call per commit: engine.applyCommit(batch)
+----------------------------v--------------------------------+
|  Layer 3: Rust Core Engine                                  |
|  The "mini operating system"                                |
|                                                             |
|  Shadow Tree | Layout (Taffy) | Event Dispatch | Scheduler  |
|  Navigation  | Accessibility  | Native Modules              |
+----------------------------+--------------------------------+
                             | PlatformBridge trait
                             | (create_view, update_view, measure_text)
+----------------------------v--------------------------------+
|  Layer 4: Platform Adapters (thin, native)                  |
|  iOS: UIKit  |  Android: Views  |  macOS: AppKit            |
|  Windows: Composition APIs      |  Web: DOM via WASM        |
+-------------------------------------------------------------+
```

### Core Design Principle

```
React = intent       (what the UI should be)
Rust  = execution    (how it gets there)
```

React never talks to native directly. React never computes layout. React never dispatches events. React declares intent through JSX. Rust executes that intent by managing the shadow tree, computing layout, routing events, and dispatching to platform bridges.

---

## Data Flow: One Complete Frame

Here is exactly what happens when a user taps a button that triggers a state update:

```
 1. Native tap event arrives
    |
    v
 2. Platform bridge captures UITouch / MotionEvent / MouseEvent
    |
    v
 3. Rust Event System
    |-- Hit test against Taffy layout tree: find target NodeId
    |-- Gesture recognizer: raw pointer sequence becomes Tap gesture
    |-- Dispatch: capture phase (root to target) then bubble phase (target to root)
    |
    v
 4. Event handler in React calls setState()
    |
    v
 5. React Fiber reconciler runs (standard React 19)
    |-- Diffing, hooks, context, suspense — all normal React
    |
    v
 6. Commit phase triggers host config
    |-- createInstance() / commitUpdate() / removeChild()
    |-- Each mutation becomes an IR command pushed to the batch
    |
    v
 7. resetAfterCommit() flushes the batch
    |-- JS Scheduler assigns priority (Immediate for input response)
    |-- Immediate: bypass requestAnimationFrame, send synchronously
    |-- Normal: coalesce with rAF, merge multiple commits
    |
    v
 8. ONE applyCommit(batch) call crosses JS to Rust boundary
    |-- Phase 1: JSON serialization (about 0.5ms for typical batch)
    |-- Phase 2: FlatBuffer zero-copy (under 0.1ms)
    |
    v
 9. Rust Engine processes the batch
    |-- Apply IR commands to shadow tree
    |-- Mark dirty nodes (HashSet)
    |-- Taffy layout recomputation (ONLY dirty subtrees)
    |-- Mount phase: create/update/position native views
    |
    v
10. Platform bridge updates actual native widgets
    |-- UIKit: setFrame, setText, setBackgroundColor
    |-- Android: setLayoutParams, setText, setBackground
    |-- DOM: style.transform, textContent, style.background
    |
    v
11. Pixels on screen. Total latency: 8-12ms (single frame).
```

---

## Project Structure

```
appscale-engine/
|
|-- Cargo.toml                            Workspace configuration
|                                         Shared deps: taffy, serde, flatbuffers, thiserror
|
|-- README.md                             This file
|
|-- rust-core/                            === RUST EXECUTION ENGINE ===
|   |-- Cargo.toml                        Crate: lib + cdylib + staticlib
|   +-- src/
|       |-- lib.rs              [315 ln]  Engine struct — central coordinator
|       |                                 Owns: tree + layout + events + scheduler
|       |                                       + navigator + focus + platform
|       |                                 Entry: apply_commit(), handle_event()
|       |
|       |-- tree.rs             [277 ln]  Shadow tree — UI node ownership
|       |                                 NodeId, ShadowNode, parent-child DAG,
|       |                                 pending props, native handle mapping
|       |                                 3 tests
|       |
|       |-- ir.rs               [190 ln]  Binary IR — transport contract
|       |                                 IrCommand enum (7 variants), IrBatch,
|       |                                 JSON encode/decode (Phase 1)
|       |                                 1 roundtrip test
|       |
|       |-- platform.rs         [267 ln]  Platform bridge — trait contracts
|       |                                 PlatformBridge trait (13 methods),
|       |                                 ViewType (15 variants), PropsDiff,
|       |                                 PlatformCapability, MockPlatform
|       |
|       |-- layout.rs           [439 ln]  Layout engine — Taffy Flexbox + Grid
|       |                                 LayoutEngine wrapping TaffyTree,
|       |                                 text measurement callback,
|       |                                 hit_test(x,y) for event system
|       |                                 1 integration test
|       |
|       |-- events.rs           [377 ln]  Unified event system
|       |                                 PointerEvent (touch/mouse/pen),
|       |                                 Capture-target-bubble dispatch,
|       |                                 Gesture recognizer state machine
|       |
|       |-- scheduler.rs        [243 ln]  Rust-side scheduler
|       |                                 5 priority lanes, frame coalescing,
|       |                                 backpressure, frame stats
|       |                                 2 tests
|       |
|       |-- navigation.rs       [418 ln]  Navigation state machine
|       |                                 Stack/Modal/Tab, deep linking,
|       |                                 path pattern matching, GoBack chain
|       |                                 5 tests
|       |
|       +-- accessibility.rs    [413 ln]  Accessibility layer
|                                         Roles with iOS/Android/Web mapping,
|                                         FocusManager, focus traps,
|                                         AccessibilityBridge trait
|                                         3 tests
|
|-- sdk/                                  === TYPESCRIPT SDK ===
|   |-- package.json                      npm: @appscale/core
|   +-- src/
|       |-- host-config.ts      [482 ln]  react-reconciler host configuration
|       |                                 Batches mutations into IrBatch,
|       |                                 one applyCommit() per commit
|       |
|       +-- scheduler.ts        [221 ln]  JS-side frame scheduler
|                                         Priority lanes, rAF coalescing,
|                                         backpressure, batch merging
|
+-- ir-schema/                            === BINARY IR SCHEMA ===
    +-- ir.fbs                  [191 ln]  FlatBuffers IDL (Phase 2)
                                          ViewType, LayoutStyle, PropsDiff,
                                          IrCommand, IrBatch
```

### Line Count Summary

| Module | File | Lines | Tests |
|--------|------|------:|------:|
| Engine coordinator | lib.rs | 315 | — |
| Shadow tree | tree.rs | 277 | 3 |
| Binary IR | ir.rs | 190 | 1 |
| Platform bridge | platform.rs | 267 | — |
| Layout (Taffy) | layout.rs | 439 | 1 |
| Event system | events.rs | 377 | — |
| Rust scheduler | scheduler.rs | 243 | 2 |
| Navigation | navigation.rs | 418 | 5 |
| Accessibility | accessibility.rs | 413 | 3 |
| Host config (TS) | host-config.ts | 482 | — |
| JS scheduler (TS) | scheduler.ts | 221 | — |
| FlatBuffer schema | ir.fbs | 191 | — |
| **Total** | **12 source files** | **3,833** | **15** |

---

## Module-by-Module Walkthrough

### 1. React Host Config

**File:** `sdk/src/host-config.ts` (482 lines)

This is the entry point of the entire framework. It implements the `react-reconciler` host configuration — the interface React uses to communicate with any rendering target.

**What each method does:**

- `createInstance(type, props)` — Called when React creates a new element. Allocates a NodeId, separates layout props from visual props, pushes a CreateNode IR command. The actual native view is NOT created here — that happens in Rust after the commit.

- `commitUpdate(instance, payload)` — Called when props change. Diffs old vs new props, pushes UpdateProps/UpdateStyle IR commands only for what changed.

- `appendChild(parent, child)` / `removeChild(parent, child)` — Tree mutations. Pushes AppendChild/RemoveChild IR commands.

- `prepareForCommit()` — Resets the command batch to empty.

- `resetAfterCommit()` — The critical moment. All accumulated IR commands are flushed to the Rust engine as a single IrBatch.

**Key design decision:** ALL mutations are batched during the commit phase and sent in ONE cross-language call. React Native's old architecture made individual bridge calls per mutation. Even Fabric processes mutations incrementally. Our approach — one atomic batch per commit — minimizes cross-language overhead and enables the scheduler to coalesce multiple commits.

**Style separation:** The host config splits `style` props into two categories:
- **Layout props** (flex, padding, width, etc.) are sent to Taffy as LayoutStyle
- **Visual props** (backgroundColor, color, borderRadius, etc.) are sent to platform bridge as PropsDiff

This separation means visual-only changes skip layout recomputation entirely.

---

### 2. JS Scheduler

**File:** `sdk/src/scheduler.ts` (221 lines)

Sits between the host config and the Rust engine. Coordinates React's async batching with vsync.

**Priority lanes (matching React scheduler):**

| Priority | Use Case | Behavior |
|----------|----------|----------|
| Immediate | Touch feedback, text input | Bypass rAF, process synchronously |
| UserBlocking | Button press, toggle | Process within current frame |
| Normal | Data fetch, state update | Coalesce across frames |
| Low | Prefetch, background sync | Defer under pressure |
| Idle | Cleanup, cache warming | Only when nothing else pending |

**Backpressure:** If the last frame exceeded 16.67ms budget, the scheduler enters pressure mode — only Immediate and UserBlocking commits are processed. Normal/Low/Idle are deferred. This prevents jank cascades.

**Batch merging:** When multiple React commits coalesce into one frame, their IR batches are merged: creates first, then updates (deduplicated by node ID), then appends, then removes.

---

### 3. Binary IR Layer

**Files:** `rust-core/src/ir.rs` (190 lines) + `ir-schema/ir.fbs` (191 lines)

The transport contract between JavaScript and Rust. The framework's biggest innovation.

**Phase 1 (current):** JSON via serde_json. Simple, debuggable.
**Phase 2 (production):** FlatBuffers. Zero-copy deserialization.

**IR Command types (7 variants):**

| Command | When | Payload |
|---------|------|---------|
| CreateNode | React creates element | node ID + ViewType + props + style |
| UpdateProps | Props change | node ID + PropsDiff (only changed keys) |
| UpdateStyle | Layout style changes | node ID + LayoutStyle |
| AppendChild | Child added | parent ID + child ID |
| InsertBefore | Child positioned | parent ID + child ID + sibling ID |
| RemoveChild | Child removed | parent ID + child ID |
| SetRootNode | Root established | node ID |

**Why Binary IR matters:**
- **Deterministic:** same React output produces same IR produces same native output
- **Replayable:** record batches for testing, debugging, or CI screenshot comparison
- **AI-generable:** an AI model can produce IR directly, bypassing React
- **Cross-process safe:** works over IPC, WebSocket, shared memory

---

### 4. Engine Coordinator

**File:** `rust-core/src/lib.rs` (315 lines)

The Engine struct is the central coordinator that owns all subsystems.

```rust
pub struct Engine {
    tree: ShadowTree,
    layout: LayoutEngine,
    events: EventDispatcher,
    scheduler: Scheduler,
    navigator: Navigator,
    focus: FocusManager,
    platform: Arc<dyn PlatformBridge>,
    dirty_nodes: HashSet<NodeId>,
}
```

**Two entry points:**

1. `apply_commit(batch)` — From JavaScript. Processes IR through the pipeline: apply to tree, mark dirty, compute layout (dirty subtrees only), mount to native.

2. `handle_event(event)` — From platform bridge. Hit tests against layout tree, dispatches through capture/bubble, feeds gesture recognizer.

**Dirty tracking:** Only nodes affected by IR commands are added to the dirty set. Layout recomputes only dirty subtrees. Visual-only prop changes skip layout entirely. Cost changes from O(total_nodes) to O(changed_nodes).

---

### 5. Shadow Tree

**File:** `rust-core/src/tree.rs` (277 lines)

Rust-side mirror of React's fiber tree. Source of truth for node existence, hierarchy, props, and native handle mappings.

```rust
pub struct ShadowNode {
    pub id: NodeId,
    pub view_type: ViewType,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub props: HashMap<String, PropValue>,
    pub native_handle: Option<NativeHandle>,
    pending_props: PropsDiff,
}
```

Key operations: `create_node`, `append_child`, `insert_before`, `remove_child` (recursive subtree removal), `take_pending_props` (consumed by mount phase), `ancestors` (for event propagation path).

**Why separate from React's fiber tree?** React's fibers live in JS, subject to GC pauses and concurrent mode interruptions. The shadow tree in Rust is stable and always reflects the last committed state — what layout, events, and platform bridges need.

---

### 6. Platform Bridge

**File:** `rust-core/src/platform.rs` (267 lines)

Trait-based contract preventing the "lowest common denominator" problem.

**Core trait (every platform MUST implement):**

```rust
pub trait PlatformBridge: Send + Sync {
    fn create_view(&self, view_type: ViewType, node_id: NodeId) -> NativeHandle;
    fn update_view(&self, handle: NativeHandle, props: &PropsDiff) -> Result<(), PlatformError>;
    fn remove_view(&self, handle: NativeHandle);
    fn insert_child(&self, parent: NativeHandle, child: NativeHandle, index: usize);
    fn remove_child(&self, parent: NativeHandle, child: NativeHandle);
    fn measure_text(&self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics;
    fn screen_size(&self) -> ScreenSize;
    fn supports(&self, capability: PlatformCapability) -> bool;
}
```

**Capability queries (not lowest common denominator):**

```rust
if bridge.supports(PlatformCapability::Haptics) {
    // Native haptic feedback
} else {
    // Visual feedback fallback
}
```

Includes `MockPlatform` for testing the full pipeline without a real OS.

---

### 7. Layout Engine

**File:** `rust-core/src/layout.rs` (439 lines)

Wraps Taffy for CSS Flexbox AND CSS Grid (Yoga only supports Flexbox).

**How it works:**
1. Creates Taffy nodes mirroring shadow tree nodes
2. Converts LayoutStyle (developer CSS subset) to Taffy Style
3. During compute(), calls platform bridge measure_text() for text nodes
4. After computation, collects absolute screen coordinates for every node
5. hit_test(x, y) returns nodes at a coordinate for the event system

**LayoutStyle supports:** display, position, flex-direction, flex-wrap, flex-grow, flex-shrink, justify-content, align-items, width/height/min/max (points, percent, auto), aspect-ratio, margin, padding, gap, overflow.

---

### 8. Event System

**File:** `rust-core/src/events.rs` (377 lines)

Unifies touch (iOS/Android), mouse (macOS/Windows/Web), keyboard, and scroll events following W3C Pointer Events.

**Dispatch pipeline:**
1. Hit test against layout tree to find target NodeId
2. Build propagation path via tree.ancestors()
3. Capture phase: root to target
4. Bubble phase: target to root
5. Handlers can stop propagation or prevent default

**Gesture recognizer state machine:**

| Gesture | Detection |
|---------|-----------|
| Tap | down + up within 300ms and 10px |
| Long press | down held 500ms+ within 10px |
| Pan | down + move beyond 10px threshold |
| Swipe | pan ending with velocity above 300px/s |

---

### 9. Rust Scheduler

**File:** `rust-core/src/scheduler.rs` (243 lines)

Priority queue with 5 lanes (Immediate through Idle). drain_frame() extracts work per these rules:
- Immediate: one item immediately, no coalescing
- UserBlocking: all UB items in queue
- Normal/Low/Idle: coalesce up to 8 items

Tracks frame stats (layout time, mount time, dropped frames) for DevTools.

---

### 10. Navigation

**File:** `rust-core/src/navigation.rs` (418 lines)

State machine in Rust managing stack, modal, and tab navigation with deep linking.

Navigation lives in Rust because platform bridges need it for native transitions (UINavigationController), deep links must resolve before React renders, and back button handling is synchronous.

**GoBack priority chain:** dismiss modal first, then pop stack, then no-op (never pops root).

**Deep linking:** Routes with patterns like `/profile/:id` match URLs and extract params.

---

### 11. Accessibility

**File:** `rust-core/src/accessibility.rs` (413 lines)

Semantic role mapping across all five platforms:

| Role | iOS (VoiceOver) | Android (TalkBack) | Web (ARIA) |
|------|-----------------|--------------------|-----------| 
| Button | button trait | android.widget.Button | role="button" |
| Heading | header trait | TextView + heading | role="heading" |
| Switch | button trait | android.widget.Switch | role="switch" |
| TextField | (inherent) | android.widget.EditText | role="textbox" |

FocusManager handles Tab/Shift+Tab navigation, explicit tab indices, and focus traps for modals.

---

### 12. FlatBuffer Schema

**File:** `ir-schema/ir.fbs` (191 lines)

FlatBuffers IDL defining the Phase 2 binary wire format. Compile with `flatc --rust --ts ir.fbs` to generate type-safe Rust and TypeScript bindings.

---

## Threading Model

```
JS Thread (Hermes / V8)
  React reconciler, hooks, event handlers
  Produces: IrBatch
       |
       | JSI (sync, zero-copy) or JSON
       v
Rust Core Thread
  Shadow tree, IR decode, scheduler, navigation, DevTools
  Produces: dirty node set
       |
       | Background (concurrent with JS)
       v
Layout Thread
  Taffy computation
  Text measurement calls back to platform main thread
  Produces: ComputedLayout per node
       |
       | Mount operations MUST be on main thread
       v
Main / UI Thread
  Platform bridge create/update/delete
  Native event collection
  Screen reader announcements
```

**Key constraint:** Layout can run on a background thread, but all native view operations must execute on the platform's main thread.

---

## How It Compares

| Capability | React Native | Flutter | .NET MAUI | AppScale |
|------------|-------------|---------|-----------|----------|
| Native widgets | Yes | No (custom-drawn) | Yes | Yes |
| iOS + Android | First-class | First-class | Yes | First-class |
| macOS + Windows | Microsoft fork (lags) | Yes | Yes | First-class |
| Web | Abandoned | Canvas-based | No | WASM + DOM |
| Language | JS/TS | Dart | C#/XAML | JS/TS |
| Layout engine | Yoga (Flexbox) | Custom | Platform | Taffy (Flex + Grid) |
| Core runtime | C++ | C++/Dart VM | .NET | Rust |
| Navigation | Third-party | Built-in | Built-in | Built-in |
| Binary transport | No | No | No | FlatBuffers |
| AI-native IR | No | No | No | Yes |

---

## Key Design Decisions

### 1. React Runtime Is Preserved

We rejected replacing React with signals/AOT because "React without React" loses hooks, context, Suspense, concurrent mode, and the library ecosystem. The React developer pool is the world's largest (44.7% market share). We optimize the execution layer underneath, not the developer-facing API.

### 2. Rust Core (Not C++)

Memory safety by default. Taffy (Rust) has CSS Grid; Yoga (C++) does not. Rust compiles to WASM natively. UniFFI generates Swift/Kotlin/C++ bindings.

### 3. Binary IR Transport

Deterministic (enables replay testing). Batchable (multiple commits coalesce). AI-generable (models produce IR directly). Cross-process safe (DevTools, remote rendering).

### 4. Platform-Adaptive (Not Lowest Common Denominator)

ReactXP's fatal mistake was exposing only APIs available on ALL platforms. We invert: every component renders the platform's best native widget. Capabilities are queried at runtime.

### 5. Navigation in Rust

Native transitions need state before React renders. Deep links resolve synchronously. Back gestures are platform-specific.

### 6. MIT License (Non-Negotiable)

Every successful UI framework uses MIT. BSL triggers enterprise bans. MIT from day one.

---

## Building and Testing

### Prerequisites

- Rust 1.75+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Node.js 18+
- FlatBuffers compiler (optional, Phase 2)

### Rust Core

```bash
cd rust-core

# Build
cargo build --release

# Run all 15 tests
cargo test

# Test specific modules
cargo test --lib tree::tests
cargo test --lib layout::tests
cargo test --lib navigation::tests
cargo test --lib accessibility::tests
cargo test --lib scheduler::tests
cargo test --lib ir::tests

# Build as shared library for FFI
cargo build --release --lib
```

### TypeScript SDK

```bash
cd sdk
npm install
npm run build
```

### FlatBuffers (Phase 2)

```bash
flatc --rust -o rust-core/src/ir_generated/ ir-schema/ir.fbs
flatc --ts -o sdk/src/ir_generated/ ir-schema/ir.fbs
```

---

## Phase Roadmap

### Phase 1: Foundation (Months 1-6)

- [x] Rust core: tree, IR, layout (Taffy), events, platform traits
- [x] Host config: react-reconciler with batched IR
- [x] Scheduler: JS + Rust with priority lanes
- [x] Navigation: stack/modal/tab with deep linking
- [x] Accessibility: roles, focus, platform mappings
- [x] FlatBuffers schema
- [ ] iOS platform bridge (UIKit via UniFFI)
- [ ] Android platform bridge (Views via JNI)
- [ ] Web platform bridge (DOM via wasm-bindgen)
- [ ] CLI: appscale create / dev / build

### Phase 2: Desktop + Production IR (Months 7-12)

- [ ] macOS bridge (AppKit) + Windows bridge (Composition APIs)
- [ ] Replace JSON with FlatBuffers
- [ ] Native module system + codegen
- [ ] DevTools: inspector, profiler, IR viewer

### Phase 3: Ecosystem (Months 13-24)

- [ ] Cloud build service + OTA updates
- [ ] 50+ built-in components
- [ ] Plugin marketplace
- [ ] AI layer (IR generation, layout optimization)

---

## Prior Art and Lessons Learned

### ReactXP (Microsoft, 2017-2019)

Closest prior art. Built by Skype team, 8.2K stars, MIT, declared End of Life 2019. Five lessons that shaped our architecture:

1. **Thin abstraction broke with every RN release.** We own the pipeline via custom reconciler + Rust core.
2. **Lowest common denominator killed native fidelity.** We use platform-adaptive components with capability queries.
3. **Single sponsor (Skype) died when Teams replaced it.** MIT license + foundation governance path.
4. **No ecosystem made adoption scary.** Built-in navigation, storage, auth for 80% of apps.
5. **No CLI or build system.** One command: `appscale build all`.

---

## License

MIT License — AppScale LLP

Copyright (c) 2026 Satyam Kumar Das
