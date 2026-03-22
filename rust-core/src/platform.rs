//! Platform Bridge — trait-based contracts for native platform integration.
//!
//! Each platform (iOS, Android, macOS, Windows, Web) implements PlatformBridge.
//! The consistency layer uses capability queries (not lowest-common-denominator)
//! to enable platform-adaptive rendering.

use crate::tree::NodeId;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core platform bridge trait
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Every platform bridge MUST implement this trait.
pub trait PlatformBridge: Send + Sync {
    fn platform_id(&self) -> PlatformId;

    // --- View lifecycle ---
    fn create_view(&self, view_type: ViewType, node_id: NodeId) -> NativeHandle;
    fn update_view(&self, handle: NativeHandle, props: &PropsDiff) -> Result<(), PlatformError>;
    fn remove_view(&self, handle: NativeHandle);
    fn insert_child(&self, parent: NativeHandle, child: NativeHandle, index: usize);
    fn remove_child(&self, parent: NativeHandle, child: NativeHandle);

    // --- Text measurement (required by Taffy for layout) ---
    fn measure_text(&self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics;

    // --- Screen info ---
    fn screen_size(&self) -> ScreenSize;
    fn scale_factor(&self) -> f32;

    // --- Capability queries ---
    fn supports(&self, capability: PlatformCapability) -> bool;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlatformId { Ios, Android, Macos, Windows, Web }

/// Opaque handle to a platform-native view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeHandle(pub u64);

/// View types the framework knows how to create.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViewType {
    Container,
    Text,
    TextInput,
    Image,
    ScrollView,
    Button,
    Switch,
    Slider,
    ActivityIndicator,
    DatePicker,
    Modal,
    BottomSheet,
    MenuBar,
    TitleBar,
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformCapability {
    Haptics,
    Biometrics,
    MenuBar,
    SystemTray,
    MultiWindow,
    DragAndDrop,
    ContextMenu,
    NativeShare,
    PushNotifications,
    BackgroundFetch,
    NativeDatePicker,
    NativeFilePicker,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Props
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A single property value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropValue {
    String(String),
    F32(f32),
    F64(f64),
    I32(i32),
    Bool(bool),
    Color(Color),
    Rect { x: f32, y: f32, width: f32, height: f32 },
    Null,
}

/// A diff of changed properties.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PropsDiff {
    pub changes: HashMap<String, PropValue>,
}

impl PropsDiff {
    pub fn new() -> Self { Self::default() }

    pub fn set(&mut self, key: impl Into<String>, value: PropValue) {
        self.changes.insert(key.into(), value);
    }

    pub fn is_empty(&self) -> bool { self.changes.is_empty() }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Color {
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Text
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub font_weight: Option<FontWeight>,
    pub color: Option<Color>,
    pub line_height: Option<f32>,
    pub letter_spacing: Option<f32>,
    pub text_align: Option<TextAlign>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FontWeight { Thin, Light, Regular, Medium, SemiBold, Bold, Heavy }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TextAlign { Left, Center, Right, Justify }

#[derive(Debug, Clone, Copy, Default)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    pub line_count: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct ScreenSize {
    pub width: f32,
    pub height: f32,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Errors
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("View not found: {0:?}")]
    ViewNotFound(NativeHandle),

    #[error("Unsupported view type: {0:?}")]
    UnsupportedViewType(ViewType),

    #[error("Native error: {0}")]
    Native(String),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test platform (for unit testing without a real OS)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A mock platform bridge for testing.
/// Records all operations for assertion.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Clone)]
    pub enum MockOp {
        CreateView(ViewType, NodeId),
        UpdateView(NativeHandle, PropsDiff),
        RemoveView(NativeHandle),
        InsertChild(NativeHandle, NativeHandle, usize),
        RemoveChild(NativeHandle, NativeHandle),
    }

    pub struct MockPlatform {
        next_handle: Mutex<u64>,
        pub ops: Mutex<Vec<MockOp>>,
    }

    impl MockPlatform {
        pub fn new() -> Self {
            Self {
                next_handle: Mutex::new(1),
                ops: Mutex::new(Vec::new()),
            }
        }
    }

    impl PlatformBridge for MockPlatform {
        fn platform_id(&self) -> PlatformId { PlatformId::Web }

        fn create_view(&self, view_type: ViewType, node_id: NodeId) -> NativeHandle {
            let mut h = self.next_handle.lock().unwrap();
            let handle = NativeHandle(*h);
            *h += 1;
            self.ops.lock().unwrap().push(MockOp::CreateView(view_type, node_id));
            handle
        }

        fn update_view(&self, handle: NativeHandle, props: &PropsDiff) -> Result<(), PlatformError> {
            self.ops.lock().unwrap().push(MockOp::UpdateView(handle, props.clone()));
            Ok(())
        }

        fn remove_view(&self, handle: NativeHandle) {
            self.ops.lock().unwrap().push(MockOp::RemoveView(handle));
        }

        fn insert_child(&self, parent: NativeHandle, child: NativeHandle, index: usize) {
            self.ops.lock().unwrap().push(MockOp::InsertChild(parent, child, index));
        }

        fn remove_child(&self, parent: NativeHandle, child: NativeHandle) {
            self.ops.lock().unwrap().push(MockOp::RemoveChild(parent, child));
        }

        fn measure_text(&self, _text: &str, style: &TextStyle, _max_width: f32) -> TextMetrics {
            let font_size = style.font_size.unwrap_or(14.0);
            TextMetrics {
                width: font_size * 0.6 * _text.len() as f32,
                height: font_size * 1.2,
                baseline: font_size,
                line_count: 1,
            }
        }

        fn screen_size(&self) -> ScreenSize {
            ScreenSize { width: 390.0, height: 844.0 }
        }

        fn scale_factor(&self) -> f32 { 3.0 }

        fn supports(&self, _cap: PlatformCapability) -> bool { false }
    }
}
