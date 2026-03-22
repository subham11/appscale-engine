# AppScale Engine

**React Native for all platforms — rebuilt for performance, AI, and scale.**

> Write React. Render native widgets. iOS, Android, macOS, Windows, Web.

## Architecture

```
JSX (Developer Code)
         ↓
React Fiber (standard React 19)
         ↓
Custom Reconciler (host-config.ts)
         ↓
Binary IR (FlatBuffers)
         ↓
Rust Core Engine (Taffy layout + event system)
         ↓
Platform Adapters (thin native bridges)
         ↓
Native Widgets (UIKit / Android Views / AppKit / WinUI / DOM)
```

## What Makes This Different

| Layer | React Native (Meta) | AppScale Engine |
|-------|---------------------|-----------------|
| Transport | JSI + Fabric C++ | Binary IR (FlatBuffers) |
| Layout | Yoga (C++, Flexbox only) | Taffy (Rust, Flexbox + CSS Grid) |
| Core | C++ (manual memory) | Rust (memory-safe, concurrent) |
| Platforms | iOS + Android (desktop = Microsoft fork) | All 5 as first-class citizens |
| Desktop | RN Windows/macOS (12-18mo lag) | Same-day releases |
| Web | Abandoned by Meta | First-class target |
| Build | Metro + Xcode + Gradle + ... | Single CLI (`appscale build all`) |

## Project Structure

```
appscale-engine/
├── Cargo.toml                    # Rust workspace root
├── rust-core/                    # Rust execution engine
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Engine coordinator
│       ├── tree.rs               # Shadow tree (UI node ownership)
│       ├── ir.rs                 # Binary IR decode/encode
│       ├── platform.rs           # Platform bridge traits + types
│       ├── layout.rs             # Taffy integration (Flexbox + Grid)
│       └── events.rs             # Unified pointer/keyboard/gesture
├── sdk/                          # TypeScript SDK (what devs import)
│   ├── package.json
│   └── src/
│       └── host-config.ts        # react-reconciler host configuration
├── ir-schema/
│   └── ir.fbs                    # FlatBuffers schema (Phase 2)
└── README.md
```

## Data Flow (One React Commit)

1. **React renders** — JSX components produce a fiber tree diff
2. **Reconciler commits** — `host-config.ts` batches mutations as IR commands
3. **Binary IR sent** — One `applyCommit(batch)` call to Rust (via JSI/WASM)
4. **Rust applies** — Shadow tree updated, Taffy computes layout
5. **Mount phase** — Platform bridge creates/positions native widgets
6. **Events flow back** — Native input → Rust dispatcher → React handlers

## Key Design Principles

- **React = intent, Rust = execution** — React describes what the UI should be; Rust manages how it gets there
- **One commit = one batch** — All mutations batched into a single cross-language call (not per-node)
- **Layout is Rust's job** — Taffy runs on a background thread, not the JS thread
- **Events flow through Rust** — Hit testing uses layout data already in Rust; no JS↔native round-trip
- **Platform-adaptive, not lowest-common-denominator** — Capability queries enable per-platform features

## Building

### Rust Core

```bash
cd rust-core
cargo build --release
cargo test
```

### TypeScript SDK

```bash
cd sdk
npm install
npm run build
```

### FlatBuffers Schema (Phase 2)

```bash
flatc --rust --ts ir-schema/ir.fbs
```

## Phase Roadmap

### Phase 1: Foundation (Current)
- [x] Rust core: shadow tree, IR, layout (Taffy), events, platform traits
- [x] React host config: react-reconciler integration
- [x] FlatBuffers IR schema
- [ ] iOS platform bridge (UIKit via UniFFI)
- [ ] Android platform bridge (Views via JNI)
- [ ] Web platform bridge (DOM via wasm-bindgen)
- [ ] CLI: `appscale create`, `appscale dev`, `appscale build`

### Phase 2: Desktop + Binary IR
- [ ] macOS platform bridge (AppKit)
- [ ] Windows platform bridge (Composition APIs)
- [ ] Replace JSON transport with FlatBuffers
- [ ] Native module system + codegen
- [ ] DevTools: tree inspector, layout overlay, profiler

### Phase 3: Ecosystem
- [ ] 50+ built-in components
- [ ] Navigation (stack, tab, drawer)
- [ ] Storage abstraction
- [ ] Cloud build service
- [ ] Plugin marketplace

## License

MIT — AppScale LLP
