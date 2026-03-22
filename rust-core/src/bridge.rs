//! Hybrid Bridge — sync reads + async mutations communication model.
//!
//! The engine's architecture is batch-oriented and frame-driven (like Flutter).
//! Enforcing fully synchronous communication would break the scheduler,
//! layout computation, and "one commit per frame" design.
//!
//! The hybrid bridge splits JS↔Rust communication into two paths:
//!
//! ┌──────────────────────────────────────────────────────┐
//! │  SYNC PATH (JSI-like, immediate return)              │
//! │  JS ↔ Rust                                           │
//! │  • Read-only queries: measure, focus, scroll offset  │
//! │  • Native event → JS callback dispatch               │
//! │  ❌ No UI mutations allowed on this path             │
//! └──────────────────────────────────────────────────────┘
//!
//! ┌──────────────────────────────────────────────────────┐
//! │  ASYNC PATH (IR / FlatBuffers, frame-batched)        │
//! │  JS → Rust (one-way per frame)                       │
//! │  • All mutations: create, update, remove, reparent   │
//! │  • Navigation, layout changes, mount/unmount         │
//! │  • Processed via scheduler priority lanes            │
//! └──────────────────────────────────────────────────────┘

use crate::layout::ComputedLayout;
use crate::navigation::NavigationAction;
use crate::platform::TextStyle;
use crate::tree::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Sync calls — read-only, immediate return
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Synchronous call from JS → Rust. Returns immediately.
/// RULE: No mutation allowed. Read-only queries only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "call")]
pub enum SyncCall {
    /// Measure a node's computed layout (x, y, width, height).
    #[serde(rename = "measure")]
    Measure { node_id: NodeId },

    /// Measure text without creating a node (for layout calculations).
    #[serde(rename = "measure_text")]
    MeasureText {
        text: String,
        #[serde(default)]
        style: TextStyleInput,
        #[serde(default = "default_max_width")]
        max_width: f32,
    },

    /// Check if a node currently has accessibility/keyboard focus.
    #[serde(rename = "is_focused")]
    IsFocused { node_id: NodeId },

    /// Get the node that currently holds focus (if any).
    #[serde(rename = "get_focused_node")]
    GetFocusedNode,

    /// Get the current scroll offset of a ScrollView node.
    #[serde(rename = "get_scroll_offset")]
    GetScrollOffset { node_id: NodeId },

    /// Query a platform capability (haptics, biometrics, etc.).
    #[serde(rename = "supports_capability")]
    SupportsCapability { capability: String },

    /// Get the current screen dimensions and scale factor.
    #[serde(rename = "get_screen_info")]
    GetScreenInfo,

    /// Check if Rust scheduler is processing (backpressure signal).
    #[serde(rename = "is_processing")]
    IsProcessing,

    /// Get the accessibility role assigned to a node.
    #[serde(rename = "get_accessibility_role")]
    GetAccessibilityRole { node_id: NodeId },

    /// Get scheduler frame stats (for DevTools).
    #[serde(rename = "get_frame_stats")]
    GetFrameStats,

    /// Check if a node exists in the shadow tree.
    #[serde(rename = "node_exists")]
    NodeExists { node_id: NodeId },

    /// Get the child count of a node.
    #[serde(rename = "get_child_count")]
    GetChildCount { node_id: NodeId },

    /// Check if back navigation is possible.
    #[serde(rename = "can_go_back")]
    CanGoBack,

    /// Get the currently active route name and params.
    #[serde(rename = "get_active_route")]
    GetActiveRoute,
}

fn default_max_width() -> f32 {
    f32::INFINITY
}

/// Simplified text style for sync MeasureText calls (serde-compatible).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextStyleInput {
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub font_weight: Option<String>,
}

impl TextStyleInput {
    pub fn to_platform_style(&self) -> TextStyle {
        use crate::platform::FontWeight;
        TextStyle {
            font_size: self.font_size,
            font_family: self.font_family.clone(),
            font_weight: self.font_weight.as_deref().and_then(|w| match w {
                "thin" => Some(FontWeight::Thin),
                "light" => Some(FontWeight::Light),
                "regular" => Some(FontWeight::Regular),
                "medium" => Some(FontWeight::Medium),
                "semibold" => Some(FontWeight::SemiBold),
                "bold" => Some(FontWeight::Bold),
                "heavy" => Some(FontWeight::Heavy),
                _ => None,
            }),
            ..Default::default()
        }
    }
}

/// Result of a synchronous call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result")]
pub enum SyncResult {
    /// Layout measurement result.
    #[serde(rename = "layout")]
    Layout {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },

    /// Text measurement result.
    #[serde(rename = "text_metrics")]
    TextMetrics {
        width: f32,
        height: f32,
        baseline: f32,
        line_count: u32,
    },

    /// Boolean result (isFocused, nodeExists, supportsCapability, isProcessing).
    #[serde(rename = "bool")]
    Bool { value: bool },

    /// Node ID result (getFocusedNode).
    #[serde(rename = "node_id")]
    NodeIdResult { node_id: Option<u64> },

    /// Scroll offset result.
    #[serde(rename = "scroll_offset")]
    ScrollOffset { x: f32, y: f32 },

    /// Screen info result.
    #[serde(rename = "screen_info")]
    ScreenInfo { width: f32, height: f32, scale: f32 },

    /// Accessibility role result.
    #[serde(rename = "role")]
    Role { role: String },

    /// Frame stats result.
    #[serde(rename = "frame_stats")]
    FrameStats {
        frame_count: u64,
        frames_dropped: u32,
        last_frame_ms: f64,
        last_layout_ms: f64,
        last_mount_ms: f64,
    },

    /// Integer result (child count, etc.).
    #[serde(rename = "int")]
    Int { value: u64 },

    /// Active route result.
    #[serde(rename = "active_route")]
    ActiveRoute {
        route_name: Option<String>,
        params: HashMap<String, String>,
    },

    /// Node not found.
    #[serde(rename = "not_found")]
    NotFound,

    /// Error result.
    #[serde(rename = "error")]
    Error { message: String },
}

impl SyncResult {
    /// Convenience: create a layout result from a ComputedLayout.
    pub fn from_layout(layout: &ComputedLayout) -> Self {
        Self::Layout {
            x: layout.x,
            y: layout.y,
            width: layout.width,
            height: layout.height,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Async calls — mutations, enqueued for next frame
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Asynchronous call from JS → Rust. Returns immediately, enqueues work.
/// These are processed on the next frame via the scheduler.
///
/// RULE: ALL mutations go through this path. Never through SyncCall.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "call")]
pub enum AsyncCall {
    /// Navigate (push, pop, modal, deep link, etc.).
    #[serde(rename = "navigate")]
    Navigate {
        action: String,
        #[serde(default)]
        route: Option<String>,
        #[serde(default)]
        params: HashMap<String, String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },

    /// Set focus to a specific node.
    #[serde(rename = "set_focus")]
    SetFocus { node_id: NodeId },

    /// Move focus in a direction (Tab, Shift+Tab).
    #[serde(rename = "move_focus")]
    MoveFocus { direction: String },

    /// Announce a message via screen reader (VoiceOver/TalkBack).
    #[serde(rename = "announce")]
    Announce { message: String },
}

impl AsyncCall {
    /// Convert a Navigate async call into a NavigationAction.
    pub fn to_navigation_action(&self) -> Option<NavigationAction> {
        match self {
            AsyncCall::Navigate {
                action,
                route,
                params,
                url,
                index,
            } => match action.as_str() {
                "push" => route.as_ref().map(|r| NavigationAction::Push {
                    route: r.clone(),
                    params: params.clone(),
                }),
                "pop" => Some(NavigationAction::Pop),
                "popToRoot" => Some(NavigationAction::PopToRoot),
                "replace" => route.as_ref().map(|r| NavigationAction::Replace {
                    route: r.clone(),
                    params: params.clone(),
                }),
                "presentModal" => route.as_ref().map(|r| NavigationAction::PresentModal {
                    route: r.clone(),
                    params: params.clone(),
                }),
                "dismissModal" => Some(NavigationAction::DismissModal),
                "switchTab" => index.map(|i| NavigationAction::SwitchTab { index: i }),
                "deepLink" => url
                    .as_ref()
                    .map(|u| NavigationAction::DeepLink { url: u.clone() }),
                "goBack" => Some(NavigationAction::GoBack),
                _ => None,
            },
            _ => None,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Native event callbacks — Rust → JS (sync dispatch)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A callback event dispatched synchronously from Rust → JS.
/// These are the native events that React needs to respond to immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum NativeCallback {
    /// Touch/pointer event on a node.
    #[serde(rename = "pointer")]
    Pointer {
        node_id: NodeId,
        event_type: String, // "down", "move", "up", "cancel"
        x: f32,
        y: f32,
    },

    /// Keyboard event.
    #[serde(rename = "keyboard")]
    Keyboard {
        node_id: NodeId,
        event_type: String, // "keydown", "keyup"
        key: String,
        code: String,
    },

    /// Focus change event.
    #[serde(rename = "focus")]
    FocusChange {
        previous: Option<NodeId>,
        current: NodeId,
    },

    /// Navigation event (back button, deep link).
    #[serde(rename = "navigation")]
    Navigation { action: String, url: Option<String> },

    /// Scroll event.
    #[serde(rename = "scroll")]
    Scroll {
        node_id: NodeId,
        offset_x: f32,
        offset_y: f32,
    },

    /// Text input change.
    #[serde(rename = "text_change")]
    TextChange { node_id: NodeId, text: String },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Compile-time enforcement
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Marker trait: types that are safe for the sync path.
/// Sync operations MUST NOT mutate the tree, layout, or native views.
///
/// This is enforced by `handle_sync` taking `&self` (not `&mut self`).
pub trait SyncSafe {}

impl SyncSafe for SyncCall {}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// C FFI entry points — called by platform bridges via JSI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Global engine pointer for FFI access.
/// Safety: Set once during initialization, read-only afterwards.
static mut ENGINE_PTR: *mut super::Engine = std::ptr::null_mut();

/// Initialize the global engine pointer. Must be called once at startup.
///
/// # Safety
/// Must be called exactly once, before any FFI calls, from the main thread.
pub unsafe fn set_engine_ptr(engine: *mut super::Engine) {
    ENGINE_PTR = engine;
}

/// Synchronous call from native → Rust. Blocks, returns JSON string.
/// The caller MUST free the returned string with `appscale_free_string()`.
///
/// # Safety
/// `json_input` must be a valid null-terminated C string.
/// `set_engine_ptr` must have been called before this function.
#[no_mangle]
pub unsafe extern "C" fn appscale_sync_call(json_input: *const c_char) -> *mut c_char {
    let c_str = unsafe { CStr::from_ptr(json_input) };
    let input = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return to_c_string_or_null(r#"{"result":"error","message":"invalid UTF-8"}"#),
    };

    let call: SyncCall = match serde_json::from_str(input) {
        Ok(c) => c,
        Err(e) => {
            let err = SyncResult::Error {
                message: format!("parse error: {}", e),
            };
            return to_c_string_or_null(&serde_json::to_string(&err).unwrap_or_default());
        }
    };

    let engine = unsafe { &*ENGINE_PTR };
    let result = engine.handle_sync(&call);
    to_c_string_or_null(&serde_json::to_string(&result).unwrap_or_default())
}

/// Asynchronous call from native → Rust. Returns immediately, enqueues work.
///
/// # Safety
/// `json_input` must be a valid null-terminated C string.
/// `set_engine_ptr` must have been called before this function.
#[no_mangle]
pub unsafe extern "C" fn appscale_async_call(json_input: *const c_char) {
    let c_str = unsafe { CStr::from_ptr(json_input) };
    let input = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };

    let call: AsyncCall = match serde_json::from_str(input) {
        Ok(c) => c,
        Err(_) => return,
    };

    let engine = unsafe { &mut *ENGINE_PTR };
    engine.handle_async(call);
}

/// Free a string returned by `appscale_sync_call`.
///
/// # Safety
/// `ptr` must have been returned by `appscale_sync_call` and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn appscale_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

/// Helper: convert a Rust string to a C string pointer (or null on failure).
fn to_c_string_or_null(s: &str) -> *mut c_char {
    CString::new(s)
        .map(|c| c.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_call_roundtrip() {
        let call = SyncCall::Measure {
            node_id: NodeId(42),
        };
        let json = serde_json::to_string(&call).unwrap();
        let decoded: SyncCall = serde_json::from_str(&json).unwrap();
        match decoded {
            SyncCall::Measure { node_id } => assert_eq!(node_id, NodeId(42)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_sync_result_roundtrip() {
        let result = SyncResult::Layout {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: SyncResult = serde_json::from_str(&json).unwrap();
        match decoded {
            SyncResult::Layout {
                x,
                y,
                width,
                height,
            } => {
                assert_eq!(x, 10.0);
                assert_eq!(y, 20.0);
                assert_eq!(width, 100.0);
                assert_eq!(height, 50.0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_native_callback_roundtrip() {
        let cb = NativeCallback::Pointer {
            node_id: NodeId(5),
            event_type: "down".into(),
            x: 150.0,
            y: 300.0,
        };
        let json = serde_json::to_string(&cb).unwrap();
        assert!(json.contains("\"event\":\"pointer\""));
        let decoded: NativeCallback = serde_json::from_str(&json).unwrap();
        match decoded {
            NativeCallback::Pointer { node_id, .. } => assert_eq!(node_id, NodeId(5)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_async_call_roundtrip() {
        // Navigate
        let call = AsyncCall::Navigate {
            action: "push".into(),
            route: Some("Settings".into()),
            params: HashMap::from([("id".into(), "42".into())]),
            url: None,
            index: None,
        };
        let json = serde_json::to_string(&call).unwrap();
        assert!(json.contains("\"call\":\"navigate\""));
        let decoded: AsyncCall = serde_json::from_str(&json).unwrap();
        match &decoded {
            AsyncCall::Navigate {
                action,
                route,
                params,
                ..
            } => {
                assert_eq!(action, "push");
                assert_eq!(route.as_deref(), Some("Settings"));
                assert_eq!(params.get("id").map(|s| s.as_str()), Some("42"));
            }
            _ => panic!("wrong variant"),
        }

        // SetFocus
        let call = AsyncCall::SetFocus {
            node_id: NodeId(99),
        };
        let json = serde_json::to_string(&call).unwrap();
        let decoded: AsyncCall = serde_json::from_str(&json).unwrap();
        match decoded {
            AsyncCall::SetFocus { node_id } => assert_eq!(node_id, NodeId(99)),
            _ => panic!("wrong variant"),
        }

        // Announce
        let call = AsyncCall::Announce {
            message: "Item added".into(),
        };
        let json = serde_json::to_string(&call).unwrap();
        let decoded: AsyncCall = serde_json::from_str(&json).unwrap();
        match decoded {
            AsyncCall::Announce { message } => assert_eq!(message, "Item added"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_new_sync_calls_roundtrip() {
        // MeasureText
        let call = SyncCall::MeasureText {
            text: "Hello".into(),
            style: TextStyleInput {
                font_size: Some(16.0),
                font_family: Some("Inter".into()),
                font_weight: None,
            },
            max_width: 200.0,
        };
        let json = serde_json::to_string(&call).unwrap();
        assert!(json.contains("\"call\":\"measure_text\""));
        let decoded: SyncCall = serde_json::from_str(&json).unwrap();
        match decoded {
            SyncCall::MeasureText {
                text,
                style,
                max_width,
            } => {
                assert_eq!(text, "Hello");
                assert_eq!(style.font_size, Some(16.0));
                assert_eq!(max_width, 200.0);
            }
            _ => panic!("wrong variant"),
        }

        // CanGoBack
        let json = r#"{"call":"can_go_back"}"#;
        let decoded: SyncCall = serde_json::from_str(json).unwrap();
        assert!(matches!(decoded, SyncCall::CanGoBack));

        // GetActiveRoute
        let json = r#"{"call":"get_active_route"}"#;
        let decoded: SyncCall = serde_json::from_str(json).unwrap();
        assert!(matches!(decoded, SyncCall::GetActiveRoute));

        // GetFocusedNode
        let json = r#"{"call":"get_focused_node"}"#;
        let decoded: SyncCall = serde_json::from_str(json).unwrap();
        assert!(matches!(decoded, SyncCall::GetFocusedNode));
    }

    #[test]
    fn test_new_sync_results_roundtrip() {
        // TextMetrics
        let result = SyncResult::TextMetrics {
            width: 80.0,
            height: 20.0,
            baseline: 16.0,
            line_count: 1,
        };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: SyncResult = serde_json::from_str(&json).unwrap();
        match decoded {
            SyncResult::TextMetrics {
                width,
                height,
                baseline,
                line_count,
            } => {
                assert_eq!(width, 80.0);
                assert_eq!(height, 20.0);
                assert_eq!(baseline, 16.0);
                assert_eq!(line_count, 1);
            }
            _ => panic!("wrong variant"),
        }

        // ActiveRoute
        let result = SyncResult::ActiveRoute {
            route_name: Some("Home".into()),
            params: HashMap::from([("tab".into(), "feed".into())]),
        };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: SyncResult = serde_json::from_str(&json).unwrap();
        match decoded {
            SyncResult::ActiveRoute { route_name, params } => {
                assert_eq!(route_name.as_deref(), Some("Home"));
                assert_eq!(params.get("tab").map(|s| s.as_str()), Some("feed"));
            }
            _ => panic!("wrong variant"),
        }

        // NodeIdResult
        let result = SyncResult::NodeIdResult { node_id: Some(42) };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: SyncResult = serde_json::from_str(&json).unwrap();
        match decoded {
            SyncResult::NodeIdResult { node_id } => assert_eq!(node_id, Some(42)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_navigate_to_action_conversion() {
        let call = AsyncCall::Navigate {
            action: "push".into(),
            route: Some("Profile".into()),
            params: HashMap::new(),
            url: None,
            index: None,
        };
        let action = call.to_navigation_action().unwrap();
        match action {
            crate::navigation::NavigationAction::Push { route, .. } => {
                assert_eq!(route, "Profile");
            }
            _ => panic!("wrong action"),
        }

        let call = AsyncCall::Navigate {
            action: "deepLink".into(),
            route: None,
            params: HashMap::new(),
            url: Some("myapp://profile/42".into()),
            index: None,
        };
        let action = call.to_navigation_action().unwrap();
        match action {
            crate::navigation::NavigationAction::DeepLink { url } => {
                assert_eq!(url, "myapp://profile/42");
            }
            _ => panic!("wrong action"),
        }

        let call = AsyncCall::SetFocus { node_id: NodeId(1) };
        assert!(call.to_navigation_action().is_none());
    }
}
