# AppScale Engine — Usage Guide

> Build cross-platform native apps with React and Rust.  
> One codebase → iOS, Android, Web, macOS, Windows.

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Quick Start (All Platforms)](#quick-start-all-platforms)
3. [Building a Web App](#building-a-web-app)
4. [Building a React Native–style Mobile App](#building-a-react-native-style-mobile-app)
5. [Building a macOS Desktop App](#building-a-macos-desktop-app)
6. [Core Concepts](#core-concepts)
7. [Component Reference](#component-reference)
8. [Navigation](#navigation)
9. [Platform-Specific Code](#platform-specific-code)
10. [Sync Bridge API](#sync-bridge-api)
11. [Native Modules (Rust)](#native-modules-rust)
12. [Project Structure Reference](#project-structure-reference)

---

## Prerequisites

| Tool         | Version  | Purpose                            |
|--------------|----------|------------------------------------|
| **Node.js**  | ≥ 18     | TypeScript SDK, bundling           |
| **Rust**     | ≥ 1.75   | Core engine compilation            |
| **npm/yarn** | latest   | Package management                 |
| **Xcode**    | ≥ 15     | macOS and iOS builds (macOS only)  |
| **wasm-pack**| latest   | Web/WASM builds                    |

Install the CLI globally:

```bash
cargo install --path cli
```

Verify:

```bash
appscale --help
```

---

## Quick Start (All Platforms)

### 1. Create a New Project

```bash
appscale create my-app
cd my-app
npm install
```

This scaffolds:

```
my-app/
├── src/
│   ├── App.tsx              # Your root component
│   ├── index.tsx            # Entry point
│   └── components/          # Your components
├── rust/
│   ├── Cargo.toml           # Native module crate
│   └── src/lib.rs           # Custom Rust modules
├── platforms/
│   ├── ios/                 # iOS project files
│   ├── android/             # Android project files
│   └── web/                 # Web config
├── package.json
└── tsconfig.json
```

### 2. The Generated App

**`src/App.tsx`** — Your root React component:

```tsx
import React from 'react';

export default function App() {
  return (
    <View style={{ flex: 1, justifyContent: 'center', alignItems: 'center' }}>
      <Text style={{ fontSize: 24, fontWeight: 'bold' }}>
        Welcome to my-app
      </Text>
      <Text style={{ fontSize: 16, color: '#666', marginTop: 8 }}>
        Built with AppScale Engine
      </Text>
    </View>
  );
}
```

**`src/index.tsx`** — The entry point:

```tsx
import { AppRegistry } from '@appscale/core';
import App from './App';

AppRegistry.registerComponent('App', () => App);
```

### 3. Run in Development

```bash
npm run dev          # Starts dev server on port 8081
```

### 4. Build for Any Platform

```bash
npm run build:web      # → dist/web/
npm run build:ios      # → dist/ios/AppScale.app
npm run build:android  # → dist/android/app.apk
npm run build:macos    # → dist/macos/AppScale.app
npm run build:windows  # → dist/windows/AppScale.exe
```

---

## Building a Web App

AppScale compiles the Rust engine to **WebAssembly** and pairs it with your React code. The result is a static site with near-native performance.

### Step-by-Step

#### 1. Create and set up the project

```bash
appscale create my-web-app
cd my-web-app
npm install
```

#### 2. Write your React UI

All standard React patterns work — hooks, context, Suspense, concurrent mode. The component API mirrors what you know from React Native, but renders to native DOM elements via the WASM bridge.

```tsx
// src/App.tsx
import React, { useState } from 'react';

export default function App() {
  const [count, setCount] = useState(0);

  return (
    <View style={{ flex: 1, padding: 20, alignItems: 'center' }}>
      <Text style={{ fontSize: 32, fontWeight: 'bold' }}>
        Counter: {count}
      </Text>

      <Button
        title="Increment"
        onPress={() => setCount(c => c + 1)}
        style={{ marginTop: 16 }}
      />

      <ScrollView style={{ flex: 1, marginTop: 24, width: '100%' }}>
        {Array.from({ length: count }, (_, i) => (
          <View key={i} style={{
            padding: 12,
            marginBottom: 8,
            backgroundColor: '#f0f0f0',
          }}>
            <Text>Item {i + 1}</Text>
          </View>
        ))}
      </ScrollView>
    </View>
  );
}
```

#### 3. Build for Web

```bash
appscale build web
# or
npm run build:web
```

**What happens under the hood:**

| Step | Tool | Output |
|------|------|--------|
| 1. Compile Rust → WASM | `wasm-pack` (target `wasm32-unknown-unknown`) | `appscale_core_bg.wasm` |
| 2. Bundle JS + WASM | Bundler | `dist/web/index.js` + `.wasm` |
| 3. Generate HTML shell | CLI | `dist/web/index.html` |

#### 4. Serve locally

```bash
npx serve dist/web
# Opens at http://localhost:3000
```

#### 5. Deploy

The `dist/web/` folder is a static site. Deploy to any static host:

```bash
# Vercel
npx vercel dist/web

# Netlify
npx netlify deploy --dir=dist/web

# GitHub Pages — copy dist/web/ to your gh-pages branch
```

### Web-Specific: Platform Capabilities

The Web platform supports these capabilities:

| Capability | Available | Notes |
|------------|-----------|-------|
| `DragAndDrop` | ✅ | HTML5 drag-and-drop API |
| `NativeShare` | ✅ | Web Share API (HTTPS required) |
| `ContextMenu` | ✅ | Browser context menu |
| `NativeFilePicker` | ✅ | `<input type="file">` |
| `Haptics` | ❌ | Not available in browsers |
| `Biometrics` | ❌ | Use WebAuthn separately |
| `MenuBar` | ❌ | No native menu bar on web |

Query at runtime:

```tsx
import { usePlatformCapability } from '@appscale/core';

function ShareButton() {
  const canShare = usePlatformCapability('NativeShare');

  if (!canShare) return null;

  return <Button title="Share" onPress={() => { /* ... */ }} />;
}
```

### Web-Specific: Screen Defaults

| Property | Default |
|----------|---------|
| Width | 1920px |
| Height | 1080px |
| Scale | 1.0x |

The engine reads the actual viewport size at runtime, and layout adapts via Taffy's Flexbox/Grid engine.

---

## Building a React Native–style Mobile App

AppScale replaces React Native's C++ bridge with Rust + Binary IR. You write the **same React/JSX** you already know. The difference is under the hood: your components compile through the AppScale reconciler → Binary IR → Rust core → native UIKit (iOS) or Android Views.

### iOS

#### 1. Create the project

```bash
appscale create my-mobile-app
cd my-mobile-app
npm install
```

#### 2. Write your app (same React code as web)

```tsx
// src/App.tsx
import React from 'react';

export default function App() {
  return (
    <View style={{ flex: 1 }}>
      <View style={{
        paddingTop: 60, paddingHorizontal: 20,
        backgroundColor: '#007AFF',
      }}>
        <Text style={{ fontSize: 28, fontWeight: 'bold', color: '#fff' }}>
          My App
        </Text>
      </View>

      <ScrollView style={{ flex: 1, padding: 16 }}>
        <TextInput
          placeholder="Search..."
          style={{ padding: 12, backgroundColor: '#f5f5f5', borderRadius: 8 }}
        />

        <View style={{ marginTop: 16 }}>
          <Text style={{ fontSize: 18 }}>Hello from AppScale!</Text>
          <Text style={{ color: '#666', marginTop: 4 }}>
            This runs natively on iOS and Android.
          </Text>
        </View>
      </ScrollView>
    </View>
  );
}
```

#### 3. Build for iOS

```bash
appscale build ios
# or
npm run build:ios
```

**What happens:**

| Step | Tool | Output |
|------|------|--------|
| 1. Compile Rust → static lib | `cargo build --target aarch64-apple-ios` | `libappscale_core.a` |
| 2. Generate Xcode project | CLI | `platforms/ios/MyApp.xcodeproj` |
| 3. Bundle JS | Bundler | `main.jsbundle` |
| 4. Build iOS app | `xcodebuild` | `dist/ios/AppScale.app` |

#### 4. Run on Simulator

```bash
# After building, open in Xcode
open platforms/ios/MyApp.xcodeproj
# Or use the dev server
appscale dev --platform ios
```

### iOS Platform Capabilities

| Capability | Available | Notes |
|------------|-----------|-------|
| `Haptics` | ✅ | UIFeedbackGenerator |
| `Biometrics` | ✅ | Face ID / Touch ID |
| `PushNotifications` | ✅ | APNs |
| `NativeShare` | ✅ | UIActivityViewController |
| `BackgroundFetch` | ✅ | BGTaskScheduler |
| `ContextMenu` | ✅ | UIContextMenuInteraction |
| `NativeDatePicker` | ✅ | UIDatePicker |
| `MenuBar` | ❌ | iOS doesn't have a menu bar |
| `SystemTray` | ❌ | iOS doesn't have a system tray |
| `MultiWindow` | ❌ | iPadOS partial via Scenes |

**Screen defaults:** 390 × 844 @3x (iPhone 15)

### Android

#### 1. Build for Android

```bash
appscale build android
# or
npm run build:android
```

**What happens:**

| Step | Tool | Output |
|------|------|--------|
| 1. Compile Rust → .so | `cargo-ndk --target aarch64-linux-android` | `libappscale_core.so` |
| 2. Build APK | Gradle | `dist/android/app.apk` |
| 3. Bundle JS | Bundler | Embedded in APK |

### Android Platform Capabilities

| Capability | Available | Notes |
|------------|-----------|-------|
| `Haptics` | ✅ | Vibrator API |
| `Biometrics` | ✅ | BiometricPrompt |
| `PushNotifications` | ✅ | FCM |
| `BackgroundFetch` | ✅ | WorkManager |
| `NativeFilePicker` | ✅ | Intent.ACTION_OPEN_DOCUMENT |
| `NativeDatePicker` | ✅ | MaterialDatePicker |
| `ContextMenu` | ✅ | PopupMenu |
| `NativeShare` | ✅ | Intent.ACTION_SEND |
| `MenuBar` | ❌ | Android uses ActionBar/Toolbar |
| `DragAndDrop` | ❌ | Use custom gestures |

**Screen defaults:** 412 × 732 @2.625x (Pixel 6, xxhdpi)

### Key Difference from React Native

| Feature | React Native | AppScale |
|---------|-------------|----------|
| Bridge | C++ JSI | Rust + Binary IR (FlatBuffers) |
| Layout | Yoga (Flexbox only) | Taffy (Flexbox + CSS Grid) |
| Batching | Per mutation | Per commit (all mutations batched) |
| Threading | UI thread + JS thread | JS → Rust core → Layout → UI thread |
| Transport | ~16ms JSON or <1ms JSI | <0.1ms FlatBuffers zero-copy |
| Navigation | Third-party (react-navigation) | Built into Rust core |
| Accessibility | Per-platform | Unified in Rust with platform mapping |

---

## Building a macOS Desktop App

AppScale produces a native macOS application using AppKit, complete with native menu bars, multi-window support, drag-and-drop, and system tray integration.

### Step-by-Step

#### 1. Create and set up

```bash
appscale create my-desktop-app
cd my-desktop-app
npm install
```

#### 2. Write your desktop UI

Use the same React primitives, with desktop-specific capabilities:

```tsx
// src/App.tsx
import React, { useState } from 'react';

export default function App() {
  return (
    <View style={{ flex: 1, flexDirection: 'row' }}>
      {/* Sidebar */}
      <View style={{
        width: 240,
        backgroundColor: '#f5f5f5',
        padding: 16,
      }}>
        <Text style={{ fontSize: 14, fontWeight: 'bold', color: '#333' }}>
          Navigation
        </Text>
        <SidebarItem label="Dashboard" />
        <SidebarItem label="Settings" />
        <SidebarItem label="About" />
      </View>

      {/* Main content */}
      <View style={{ flex: 1, padding: 24 }}>
        <Text style={{ fontSize: 28, fontWeight: 'bold' }}>
          Dashboard
        </Text>
        <Text style={{ fontSize: 14, color: '#888', marginTop: 8 }}>
          Running natively on macOS with AppKit
        </Text>

        <View style={{
          marginTop: 24,
          flexDirection: 'row',
          gap: 16,
          flexWrap: 'wrap',
        }}>
          <StatCard title="CPU" value="23%" />
          <StatCard title="Memory" value="4.2 GB" />
          <StatCard title="Disk" value="128 GB free" />
        </View>
      </View>
    </View>
  );
}

function SidebarItem({ label }: { label: string }) {
  return (
    <View style={{ padding: 8, marginTop: 4 }}>
      <Text style={{ fontSize: 13 }}>{label}</Text>
    </View>
  );
}

function StatCard({ title, value }: { title: string; value: string }) {
  return (
    <View style={{
      padding: 16, minWidth: 150,
      backgroundColor: '#fff',
      borderRadius: 8,
    }}>
      <Text style={{ fontSize: 12, color: '#888' }}>{title}</Text>
      <Text style={{ fontSize: 24, fontWeight: 'bold', marginTop: 4 }}>{value}</Text>
    </View>
  );
}
```

#### 3. Build for macOS

```bash
appscale build macos
# or
npm run build:macos
```

**What happens:**

| Step | Tool | Output |
|------|------|--------|
| 1. Compile Rust → dylib | `cargo build --target aarch64-apple-darwin` | `libappscale_core.dylib` |
| 2. Generate Xcode project | CLI (macOS target) | `platforms/macos/MyApp.xcodeproj` |
| 3. Bundle JS | Bundler | Embedded in `.app` bundle |
| 4. Build .app | `xcodebuild` | `dist/macos/AppScale.app` |

#### 4. Run

```bash
open dist/macos/AppScale.app
# Or during development:
appscale dev --platform macos
```

### macOS Platform Capabilities

| Capability | Available | Notes |
|------------|-----------|-------|
| `MenuBar` | ✅ | Native NSMenu integration |
| `SystemTray` | ✅ | NSStatusItem in menu bar |
| `MultiWindow` | ✅ | Multiple NSWindow instances |
| `DragAndDrop` | ✅ | NSDraggingDestination |
| `ContextMenu` | ✅ | Right-click NSMenu |
| `NativeFilePicker` | ✅ | NSOpenPanel / NSSavePanel |
| `NativeDatePicker` | ✅ | NSDatePicker |
| `Haptics` | ❌ | No haptic hardware on Mac |
| `Biometrics` | ❌ | Use Touch ID separately via LAContext |
| `PushNotifications` | ❌ | Use UserNotifications separately |

**Screen defaults:** 1512 × 982 @2x (14" MacBook Pro Retina)

### macOS-Specific Patterns

**Multi-window support** — query at runtime:

```tsx
import { usePlatformCapability } from '@appscale/core';

function App() {
  const hasMultiWindow = usePlatformCapability('MultiWindow');

  return (
    <View style={{ flex: 1 }}>
      {hasMultiWindow && (
        <Button title="Open New Window" onPress={openNewWindow} />
      )}
    </View>
  );
}
```

**System tray app** — detect availability:

```tsx
const hasSystemTray = usePlatformCapability('SystemTray');
```

**Native file picker**:

```tsx
const hasFilePicker = usePlatformCapability('NativeFilePicker');
```

---

## Core Concepts

### How Rendering Works

```
Your JSX Components
    ↓ React reconciler diffs the fiber tree
    ↓ Commit phase batches all mutations
IR Commands (create, update, append, remove)
    ↓ Sent to Rust core as a single batch
Rust Engine
    ↓ Updates shadow tree
    ↓ Runs Taffy layout (only dirty subtrees)
    ↓ Mounts native views via PlatformBridge
Native Platform (UIKit / Android Views / DOM / AppKit / WinUI)
```

**Key insight:** Mutations are never sent individually. React's commit phase collects all changes into one `IrBatch`, which Rust processes atomically. This eliminates the "one bridge call per mutation" bottleneck of older architectures.

### Layout Engine

AppScale uses **Taffy** for layout, which supports both **Flexbox** and **CSS Grid**:

```tsx
// Flexbox (same as React Native)
<View style={{
  flexDirection: 'row',
  justifyContent: 'space-between',
  alignItems: 'center',
  padding: 16,
  gap: 8,
}}>
  <Text>Left</Text>
  <Text>Right</Text>
</View>

// CSS Grid (not available in React Native!)
<View style={{
  display: 'grid',
  gridTemplateColumns: '1fr 1fr 1fr',
  gridTemplateRows: 'auto',
  gap: 16,
}}>
  <View><Text>Cell 1</Text></View>
  <View><Text>Cell 2</Text></View>
  <View><Text>Cell 3</Text></View>
</View>
```

### Scheduler & Priority

The framework has 5 priority lanes. When the frame budget (16.67ms) is exceeded, lower-priority work is deferred:

| Priority | Use Case | Behavior |
|----------|----------|----------|
| **Immediate** | Crash-level updates | Flushed synchronously |
| **UserBlocking** | Button press, input | Always processed |
| **Normal** | Data fetch results | Processed if budget allows |
| **Low** | Analytics, prefetch | Deferred under pressure |
| **Idle** | Off-screen prerender | Only when idle |

---

## Component Reference

### Available Components

| Component | View Type | Description |
|-----------|-----------|-------------|
| `<View>` | `Container` | Flexbox/Grid container |
| `<Text>` | `Text` | Text display |
| `<TextInput>` | `TextInput` | Text input field |
| `<Image>` | `Image` | Image display |
| `<ScrollView>` | `ScrollView` | Scrollable container |
| `<Button>` | `Button` | Pressable button |
| `<Switch>` | `Switch` | Toggle switch |
| `<Slider>` | `Slider` | Value slider |
| `<ActivityIndicator>` | `ActivityIndicator` | Loading spinner |
| `<DatePicker>` | `DatePicker` | Date selection |
| `<Modal>` | `Modal` | Modal overlay |
| `<BottomSheet>` | `BottomSheet` | Bottom sheet panel |

### List Components

| Component | Description | Key Props |
|-----------|-------------|-----------|
| `<FlatList>` | Virtualized flat list | `initialNumToRender`, `windowSize`, `removeClippedSubviews` |
| `<SectionList>` | Grouped/sectioned list | Same as FlatList + section headers |

### Navigation Components

| Component | Description |
|-----------|-------------|
| `<StackNavigator>` | Push/pop screen stack |
| `<TabNavigator>` | Bottom/top tab bar |
| `<DrawerNavigator>` | Slide-out drawer |

---

## Navigation

Navigation is built into the Rust core (not a JS-only library). This enables native transitions, synchronous deep link resolution, and platform-native back gestures.

### Using the Navigation Hook

```tsx
import { useNavigation } from '@appscale/core';

function HomeScreen() {
  const { route, canGoBack, push, goBack } = useNavigation();

  return (
    <View style={{ flex: 1, padding: 20 }}>
      <Text style={{ fontSize: 24 }}>Current: {route}</Text>

      <Button
        title="Go to Profile"
        onPress={() => push('/profile/123')}
      />

      {canGoBack && (
        <Button title="Go Back" onPress={goBack} />
      )}
    </View>
  );
}
```

### Navigation Actions

| Action | Description |
|--------|-------------|
| `push(path)` | Push a new screen onto the stack |
| `pop()` | Pop the top screen |
| `replace(path)` | Replace the current screen |
| `goBack()` | Go back (pop or dismiss modal) |
| `presentModal(path)` | Present a modal screen |
| `dismissModal()` | Dismiss the top modal |
| `switchTab(index)` | Switch to a tab by index |
| `deepLink(url)` | Resolve a deep link URL |
| `popToRoot()` | Pop to the root of the stack |

### Deep Linking

The Rust navigator supports path patterns with parameters:

```
/profile/:id        →  matches /profile/123
/settings           →  matches /settings
/post/:postId/comments/:commentId  →  matches /post/42/comments/7
```

---

## Platform-Specific Code

### Runtime Capability Checks

Instead of `Platform.OS === 'ios'`, AppScale uses capability queries:

```tsx
import { usePlatformCapability, useScreenSize } from '@appscale/core';

function AdaptiveLayout() {
  const hasMenuBar = usePlatformCapability('MenuBar');  // true on macOS
  const hasBiometrics = usePlatformCapability('Biometrics'); // true on iOS/Android
  const screen = useScreenSize();

  return (
    <View style={{ flex: 1, flexDirection: screen.width > 768 ? 'row' : 'column' }}>
      {hasMenuBar && <MenuBarComponent />}
      <MainContent />
      {hasBiometrics && <BiometricLoginButton />}
    </View>
  );
}
```

### Capability Matrix

| Capability | iOS | Android | Web | macOS | Windows |
|------------|-----|---------|-----|-------|---------|
| `Haptics` | ✅ | ✅ | ❌ | ❌ | ❌ |
| `Biometrics` | ✅ | ✅ | ❌ | ❌ | ❌ |
| `MenuBar` | ❌ | ❌ | ❌ | ✅ | ✅ |
| `SystemTray` | ❌ | ❌ | ❌ | ✅ | ✅ |
| `MultiWindow` | ❌ | ❌ | ❌ | ✅ | ✅ |
| `DragAndDrop` | ❌ | ❌ | ✅ | ✅ | ✅ |
| `NativeShare` | ✅ | ✅ | ✅ | ❌ | ❌ |
| `PushNotifications` | ✅ | ✅ | ❌ | ❌ | ❌ |
| `BackgroundFetch` | ✅ | ✅ | ❌ | ❌ | ❌ |
| `ContextMenu` | ✅ | ✅ | ✅ | ✅ | ❌ |
| `NativeFilePicker` | ❌ | ✅ | ✅ | ✅ | ❌ |
| `NativeDatePicker` | ✅ | ✅ | ❌ | ✅ | ❌ |

---

## Sync Bridge API

The sync bridge provides instant (<0.1ms) read-only access to engine state. These never mutate the UI.

```tsx
import {
  measure, isFocused, getScrollOffset, supports,
  getScreenSize, isProcessing, nodeExists, getChildCount,
  canGoBack, getActiveRoute, getFrameStats,
} from '@appscale/core';

// Layout measurement
const layout = measure(nodeId);
// → { x: 0, y: 100, width: 390, height: 44 } | null

// Focus state
const focused = isFocused(nodeId);        // → boolean
const focusedNode = getFocusedNode();      // → nodeId | null

// Scroll position
const scroll = getScrollOffset(scrollViewId);
// → { x: 0, y: 120 }

// Screen info
const screen = getScreenSize();
// → { width: 390, height: 844, scale: 3 }

// Engine diagnostics
const stats = getFrameStats();
// → { frame_count, frames_dropped, last_frame_ms, last_layout_ms, last_mount_ms }
```

---

## Native Modules (Rust)

Add custom Rust code for performance-critical tasks. These are accessible from JS via the bridge.

### Writing a Native Module

Edit `rust/src/lib.rs` in your project:

```rust
//! Custom native modules for my-app.

use appscale_core::modules::{NativeModule, ModuleMeta, ModuleResult};

pub struct CryptoModule;

impl NativeModule for CryptoModule {
    fn meta(&self) -> ModuleMeta {
        ModuleMeta {
            name: "Crypto".into(),
            version: "1.0.0".into(),
        }
    }

    fn call(&self, method: &str, args: &[u8]) -> ModuleResult {
        match method {
            "hash" => {
                // Your custom logic
                ModuleResult::success(b"hashed_value")
            }
            _ => ModuleResult::not_found(),
        }
    }
}

pub fn init() {
    // Register your modules at startup
}
```

---

## Project Structure Reference

```
my-app/
├── src/                        # Your React code (shared across ALL platforms)
│   ├── App.tsx                 # Root component
│   ├── index.tsx               # Entry point (AppRegistry.registerComponent)
│   └── components/             # Reusable components
│
├── rust/                       # Custom native modules (Rust)
│   ├── Cargo.toml              # Depends on appscale-core
│   └── src/lib.rs              # Your native module implementations
│
├── platforms/                  # Platform-specific configuration
│   ├── ios/                    # Xcode project, Info.plist, etc.
│   ├── android/                # Gradle config, AndroidManifest, etc.
│   └── web/                    # Webpack/Vite config, index.html template
│
├── dist/                       # Build output (generated)
│   ├── web/                    # Static files (index.html + WASM + JS)
│   ├── ios/                    # AppScale.app
│   ├── android/                # app.apk
│   ├── macos/                  # AppScale.app
│   └── windows/                # AppScale.exe
│
├── package.json                # @appscale/core + react peer deps
├── tsconfig.json               # Strict TypeScript config
└── .gitignore
```

### Build Targets

| Platform | Rust Target | Output Type | Library Format |
|----------|-------------|-------------|----------------|
| **Web** | `wasm32-unknown-unknown` | Static site | `.wasm` module |
| **iOS** | `aarch64-apple-ios` | `.app` bundle | `.a` (static lib) |
| **Android** | `aarch64-linux-android` | `.apk` | `.so` (shared lib) |
| **macOS** | `aarch64-apple-darwin` | `.app` bundle | `.dylib` |
| **Windows** | `x86_64-pc-windows-msvc` | `.exe` | `.dll` |

---

## Quick Reference: Cross-Platform Checklist

Before shipping to all platforms, verify:

- [ ] **Layout** — Test with different screen sizes (`useScreenSize()`)
- [ ] **Capabilities** — Guard platform-specific features with `usePlatformCapability()`
- [ ] **Navigation** — Test deep links resolve on all targets
- [ ] **Text** — Font sizing renders correctly (SF Pro on Apple, Roboto on Android, system-ui on Web)
- [ ] **Touch vs. Mouse** — Test pointer events on both touch (mobile) and mouse (desktop/web)
- [ ] **Performance** — Check `getFrameStats()` for dropped frames on lower-end devices
- [ ] **Accessibility** — Verify focus traversal (Tab/Shift+Tab) works on desktop, VoiceOver on iOS

---

*For architecture details, see [AppScale_Engine_README.md](AppScale_Engine_README.md).*  
*For implementation status, see [plan.md](plan.md).*
