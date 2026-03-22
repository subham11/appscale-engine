//! AppScale Universal Framework — Rust Execution Engine
//!
//! This is the "mini operating system" that sits between React's reconciler
//! and platform-native widgets. It owns:
//! - UI tree lifecycle (shadow tree)
//! - Layout computation (Taffy)
//! - Event routing and gesture recognition
//! - Binary IR decode/encode
//! - Platform bridge dispatch
//!
//! Design principle: React = intent, Rust = execution.

pub mod accessibility;
pub mod ai;
pub mod bridge;
pub mod cloud;
pub mod components;
pub mod devtools;
pub mod events;
pub mod generated;
pub mod ir;
pub mod layout;
pub mod modules;
pub mod navigation;
pub mod platform;
pub mod platform_android;
pub mod platform_ios;
pub mod platform_macos;
pub mod platform_web;
pub mod platform_windows;
pub mod plugins;
pub mod scheduler;
pub mod storage;
pub mod tree;

/// Re-export core types used across the engine.
pub mod prelude {
    pub use crate::accessibility::{AccessibilityInfo, AccessibilityRole, FocusManager};
    pub use crate::bridge::{AsyncCall, NativeCallback, SyncCall, SyncResult};
    pub use crate::events::{InputEvent, KeyboardEvent, PointerEvent};
    pub use crate::ir::{IrBatch, IrCommand};
    pub use crate::layout::{ComputedLayout, LayoutEngine, LayoutStyle};
    pub use crate::navigation::{NavigationAction, NavigationEvent, Navigator};
    pub use crate::platform::{
        NativeHandle, PlatformBridge, PlatformCapability, PlatformId, PropValue, PropsDiff,
        ViewType,
    };
    pub use crate::platform_android::AndroidPlatform;
    pub use crate::platform_ios::IosPlatform;
    pub use crate::platform_macos::MacosPlatform;
    pub use crate::platform_web::WebPlatform;
    pub use crate::platform_windows::WindowsPlatform;
    pub use crate::scheduler::{Priority, Scheduler};
    pub use crate::tree::{NodeId, ShadowTree};
}

use accessibility::FocusManager;
use bridge::{AsyncCall, SyncCall, SyncResult};
use events::{EventDispatcher, InputEvent};
use layout::LayoutEngine;
use navigation::Navigator;
use platform::PlatformBridge;
use scheduler::Scheduler;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use tree::ShadowTree;

/// The Engine is the central coordinator.
/// One Engine instance per application.
pub struct Engine {
    tree: ShadowTree,
    layout: LayoutEngine,
    events: EventDispatcher,
    scheduler: Scheduler,
    navigator: Navigator,
    focus: FocusManager,
    platform: Arc<dyn PlatformBridge>,
    frame_count: u64,

    /// Dirty tracking: nodes whose layout needs recomputation.
    /// Only dirty subtrees are recomputed — not the full tree.
    dirty_nodes: HashSet<tree::NodeId>,
}

impl Engine {
    /// Create a new Engine with the given platform bridge.
    pub fn new(platform: Arc<dyn PlatformBridge>) -> Self {
        tracing::info!(
            platform = ?platform.platform_id(),
            "AppScale Engine initialized"
        );

        Self {
            tree: ShadowTree::new(),
            layout: LayoutEngine::new(),
            events: EventDispatcher::new(),
            scheduler: Scheduler::new(),
            navigator: Navigator::new(),
            focus: FocusManager::new(),
            platform,
            frame_count: 0,
            dirty_nodes: HashSet::new(),
        }
    }

    /// Process a batch of IR commands from the reconciler.
    /// This is the main entry point — called after every React commit.
    ///
    /// The flow:
    /// 1. Decode IR commands (create/update/delete/move nodes)
    /// 2. Apply to shadow tree + mark dirty
    /// 3. Recompute layout ONLY for dirty subtrees (Taffy)
    /// 4. Diff against previous layout
    /// 5. Issue platform bridge calls (mount phase)
    pub fn apply_commit(&mut self, batch: &ir::IrBatch) -> Result<(), EngineError> {
        self.frame_count += 1;
        let _frame_start = Instant::now();
        let _span = tracing::info_span!("commit", frame = self.frame_count).entered();

        // Phase 1: Apply IR commands to shadow tree + collect dirty nodes
        let dirty_nodes = self.apply_ir_to_tree(batch)?;
        self.dirty_nodes.extend(&dirty_nodes);

        // Phase 2: Recompute layout for dirty subtrees only
        let layout_start = Instant::now();
        if !self.dirty_nodes.is_empty() {
            let screen_size = self.platform.screen_size();
            self.layout.compute(
                &self.tree,
                screen_size.width,
                screen_size.height,
                &*self.platform,
            )?;
        }
        let layout_duration = layout_start.elapsed();

        // Phase 3: Mount — apply changes to native views
        let mount_start = Instant::now();
        let dirty_vec: Vec<_> = self.dirty_nodes.drain().collect();
        self.mount_changes(&dirty_vec)?;
        let mount_duration = mount_start.elapsed();

        // Record frame stats for DevTools
        self.scheduler.record_frame(
            layout_duration,
            mount_duration,
            1, // batches processed
        );

        Ok(())
    }

    /// Enqueue a batch via the scheduler (called from JS via JSI).
    /// The scheduler handles priority ordering and frame coalescing.
    pub fn enqueue_commit(&self, batch: ir::IrBatch, priority: scheduler::Priority) {
        self.scheduler.enqueue(batch, priority);
    }

    /// Process all pending scheduled work for this frame.
    /// Called by the platform's vsync/display-link callback.
    pub fn process_frame(&mut self) -> Result<(), EngineError> {
        let batches = self.scheduler.drain_frame();
        for batch in &batches {
            self.apply_commit(batch)?;
        }
        Ok(())
    }

    /// Handle a navigation action.
    pub fn navigate(
        &mut self,
        action: navigation::NavigationAction,
    ) -> Vec<navigation::NavigationEvent> {
        self.navigator.dispatch(action)
    }

    /// Get the navigator (for DevTools inspection).
    pub fn navigator(&self) -> &Navigator {
        &self.navigator
    }

    /// Get the focus manager.
    pub fn focus_manager(&mut self) -> &mut FocusManager {
        &mut self.focus
    }

    /// Get scheduler stats (for DevTools).
    pub fn scheduler_stats(&self) -> scheduler::FrameStats {
        self.scheduler.stats()
    }

    /// Apply IR commands to the shadow tree. Returns IDs of nodes that changed.
    fn apply_ir_to_tree(&mut self, batch: &ir::IrBatch) -> Result<Vec<tree::NodeId>, EngineError> {
        let mut dirty = Vec::new();

        for cmd in &batch.commands {
            match cmd {
                ir::IrCommand::CreateNode {
                    id,
                    view_type,
                    props,
                    style,
                } => {
                    self.tree.create_node(*id, view_type.clone(), props.clone());
                    self.layout.create_node(*id, style)?;
                    dirty.push(*id);
                }
                ir::IrCommand::UpdateProps { id, diff } => {
                    self.tree.update_props(*id, diff);
                    dirty.push(*id);
                }
                ir::IrCommand::UpdateStyle { id, style } => {
                    self.layout.update_style(*id, style)?;
                    dirty.push(*id);
                }
                ir::IrCommand::AppendChild { parent, child } => {
                    self.tree.append_child(*parent, *child);
                    self.layout.set_children_from_tree(*parent, &self.tree)?;
                    dirty.push(*parent);
                }
                ir::IrCommand::InsertBefore {
                    parent,
                    child,
                    before,
                } => {
                    self.tree.insert_before(*parent, *child, *before);
                    self.layout.set_children_from_tree(*parent, &self.tree)?;
                    dirty.push(*parent);
                }
                ir::IrCommand::RemoveChild { parent, child } => {
                    self.tree.remove_child(*parent, *child);
                    self.layout.remove_node(*child);
                    dirty.push(*parent);
                    dirty.push(*child);
                }
                ir::IrCommand::SetRootNode { id } => {
                    self.tree.set_root(*id);
                    self.layout.set_root(*id);
                    dirty.push(*id);
                }
            }
        }

        Ok(dirty)
    }

    /// Mount phase: create/update/position native views.
    fn mount_changes(&mut self, dirty_nodes: &[tree::NodeId]) -> Result<(), EngineError> {
        for &node_id in dirty_nodes {
            let (view_type, parent_info, native_handle) = match self.tree.get(node_id) {
                Some(n) => (
                    n.view_type.clone(),
                    n.parent.and_then(|pid| {
                        self.tree.get(pid).and_then(|p| {
                            p.native_handle.map(|h| {
                                (
                                    h,
                                    p.children.iter().position(|&c| c == node_id).unwrap_or(0),
                                )
                            })
                        })
                    }),
                    n.native_handle,
                ),
                None => continue, // Node was removed
            };

            // Create native view if it doesn't exist yet
            if native_handle.is_none() {
                let handle = self.platform.create_view(view_type, node_id);
                self.tree.set_native_handle(node_id, handle);

                // If this node has a parent, insert into parent's native view
                if let Some((parent_handle, index)) = parent_info {
                    self.platform.insert_child(parent_handle, handle, index);
                }
            }

            // Apply props to native view
            if let Some(handle) = self.tree.get(node_id).and_then(|n| n.native_handle) {
                let props_diff = self.tree.take_pending_props(node_id);
                if !props_diff.is_empty() {
                    self.platform
                        .update_view(handle, &props_diff)
                        .map_err(|e| EngineError::Platform(e.to_string()))?;
                }

                // Apply computed layout position
                if let Some(layout) = self.layout.get_computed(node_id) {
                    let mut position_props = platform::PropsDiff::new();
                    position_props.set(
                        "frame",
                        PropValue::Rect {
                            x: layout.x,
                            y: layout.y,
                            width: layout.width,
                            height: layout.height,
                        },
                    );
                    self.platform
                        .update_view(handle, &position_props)
                        .map_err(|e| EngineError::Platform(e.to_string()))?;
                }
            }
        }

        Ok(())
    }

    /// Handle a native input event.
    /// Called by the platform bridge when the OS delivers touch/mouse/keyboard events.
    pub fn handle_event(&mut self, event: InputEvent) -> events::EventResult {
        self.events.dispatch(event, &self.layout, &self.tree)
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Sync path — read-only queries, takes &self (not &mut self)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Handle a synchronous call from JS.
    /// This takes `&self` — compile-time guarantee that no mutation happens.
    ///
    /// RULE: No UI mutation allowed on the sync path.
    pub fn handle_sync(&self, call: &SyncCall) -> SyncResult {
        match call {
            SyncCall::Measure { node_id } => match self.layout.get_computed(*node_id) {
                Some(layout) => SyncResult::from_layout(layout),
                None => SyncResult::NotFound,
            },

            SyncCall::IsFocused { node_id } => SyncResult::Bool {
                value: self.focus.focused() == Some(*node_id),
            },

            SyncCall::GetScrollOffset { node_id } => {
                // Scroll offset is tracked per-node; placeholder until
                // ScrollView tracking is wired up in the event system.
                if self.tree.get(*node_id).is_some() {
                    SyncResult::ScrollOffset { x: 0.0, y: 0.0 }
                } else {
                    SyncResult::NotFound
                }
            }

            SyncCall::SupportsCapability { capability } => {
                let cap = match capability.as_str() {
                    "haptics" => Some(platform::PlatformCapability::Haptics),
                    "biometrics" => Some(platform::PlatformCapability::Biometrics),
                    "menuBar" => Some(platform::PlatformCapability::MenuBar),
                    "systemTray" => Some(platform::PlatformCapability::SystemTray),
                    "multiWindow" => Some(platform::PlatformCapability::MultiWindow),
                    "dragAndDrop" => Some(platform::PlatformCapability::DragAndDrop),
                    "contextMenu" => Some(platform::PlatformCapability::ContextMenu),
                    "nativeShare" => Some(platform::PlatformCapability::NativeShare),
                    "pushNotifications" => Some(platform::PlatformCapability::PushNotifications),
                    "backgroundFetch" => Some(platform::PlatformCapability::BackgroundFetch),
                    "nativeDatePicker" => Some(platform::PlatformCapability::NativeDatePicker),
                    "nativeFilePicker" => Some(platform::PlatformCapability::NativeFilePicker),
                    _ => None,
                };
                SyncResult::Bool {
                    value: cap.is_some_and(|c| self.platform.supports(c)),
                }
            }

            SyncCall::GetScreenInfo => {
                let size = self.platform.screen_size();
                SyncResult::ScreenInfo {
                    width: size.width,
                    height: size.height,
                    scale: self.platform.scale_factor(),
                }
            }

            SyncCall::IsProcessing => SyncResult::Bool {
                value: self.scheduler.is_processing(),
            },

            SyncCall::GetAccessibilityRole { node_id } => {
                // Accessibility role lookup from shadow tree node props.
                // Full a11y info will be richer once AccessibilityInfo is
                // stored per-node; for now return based on view type.
                match self.tree.get(*node_id) {
                    Some(node) => {
                        let role = match &node.view_type {
                            platform::ViewType::Button => "button",
                            platform::ViewType::Text => "text",
                            platform::ViewType::TextInput => "textField",
                            platform::ViewType::Image => "image",
                            platform::ViewType::Switch => "switch",
                            platform::ViewType::Slider => "adjustable",
                            _ => "none",
                        };
                        SyncResult::Role {
                            role: role.to_string(),
                        }
                    }
                    None => SyncResult::NotFound,
                }
            }

            SyncCall::GetFrameStats => {
                let stats = self.scheduler.stats();
                SyncResult::FrameStats {
                    frame_count: stats.frame_count,
                    frames_dropped: stats.frames_dropped,
                    last_frame_ms: stats.last_frame_duration.as_secs_f64() * 1000.0,
                    last_layout_ms: stats.last_layout_duration.as_secs_f64() * 1000.0,
                    last_mount_ms: stats.last_mount_duration.as_secs_f64() * 1000.0,
                }
            }

            SyncCall::NodeExists { node_id } => SyncResult::Bool {
                value: self.tree.get(*node_id).is_some(),
            },

            SyncCall::GetChildCount { node_id } => match self.tree.get(*node_id) {
                Some(node) => SyncResult::Int {
                    value: node.children.len() as u64,
                },
                None => SyncResult::NotFound,
            },

            SyncCall::MeasureText {
                text,
                style,
                max_width,
            } => {
                let platform_style = style.to_platform_style();
                let metrics = self
                    .platform
                    .measure_text(text, &platform_style, *max_width);
                SyncResult::TextMetrics {
                    width: metrics.width,
                    height: metrics.height,
                    baseline: metrics.baseline,
                    line_count: metrics.line_count,
                }
            }

            SyncCall::GetFocusedNode => SyncResult::NodeIdResult {
                node_id: self.focus.focused().map(|id| id.0),
            },

            SyncCall::CanGoBack => SyncResult::Bool {
                value: self.navigator.can_go_back(),
            },

            SyncCall::GetActiveRoute => match self.navigator.active_screen() {
                Some(screen) => SyncResult::ActiveRoute {
                    route_name: Some(screen.route_name.clone()),
                    params: screen.params.clone(),
                },
                None => SyncResult::ActiveRoute {
                    route_name: None,
                    params: std::collections::HashMap::new(),
                },
            },
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Async path — mutations, enqueued for next frame
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Handle an asynchronous call from JS.
    /// This takes `&mut self` — mutations are allowed.
    pub fn handle_async(&mut self, call: AsyncCall) {
        match &call {
            AsyncCall::Navigate { .. } => {
                if let Some(action) = call.to_navigation_action() {
                    self.navigator.dispatch(action);
                }
            }
            AsyncCall::SetFocus { node_id } => {
                let _ = self.focus.focus(*node_id);
            }
            AsyncCall::MoveFocus { direction } => {
                let _direction = direction; // TODO: directional focus traversal
                                            // For now, just clear focus as placeholder
            }
            AsyncCall::Announce { message } => {
                tracing::info!(message = %message, "a11y announcement");
                // Platform-specific announcement will be routed through
                // the platform bridge once the announce() method is added.
            }
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Accessors (DevTools)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Get the shadow tree (for DevTools inspection).
    pub fn tree(&self) -> &ShadowTree {
        &self.tree
    }

    /// Get the layout engine (for DevTools layout overlay).
    pub fn layout(&self) -> &LayoutEngine {
        &self.layout
    }
}

use platform::PropValue;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Layout error: {0}")]
    Layout(#[from] layout::LayoutError),

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("IR decode error: {0}")]
    IrDecode(String),

    #[error("Node not found: {0:?}")]
    NodeNotFound(tree::NodeId),
}
