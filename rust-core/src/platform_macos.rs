//! macOS Desktop Platform Bridge
//!
//! Implements PlatformBridge for macOS using AppKit concepts.
//! At this stage, this is a scaffold that records operations and will
//! later connect to the actual Objective-C/Swift bridge via FFI.
//!
//! Architecture:
//!   Rust PlatformBridge → FFI (C ABI) → Swift/ObjC AppKit wrapper
//!
//! The macOS bridge handles NSView hierarchy, NSTextField measurement,
//! and macOS-specific capabilities like MenuBar, SystemTray, and DragAndDrop.

use crate::platform::*;
use crate::tree::NodeId;
use std::sync::Mutex;
use std::collections::HashMap;

/// macOS platform bridge.
///
/// In production, each method will call through FFI to the Swift/ObjC layer.
/// For now, we maintain an in-memory view registry for testing and development.
pub struct MacosPlatform {
    next_handle: Mutex<u64>,
    views: Mutex<HashMap<u64, ViewRecord>>,
    screen: ScreenSize,
    scale: f32,
}

struct ViewRecord {
    view_type: ViewType,
    node_id: NodeId,
    children: Vec<NativeHandle>,
}

impl MacosPlatform {
    pub fn new() -> Self {
        Self {
            next_handle: Mutex::new(1),
            views: Mutex::new(HashMap::new()),
            // Default: 14" MacBook Pro logical resolution
            screen: ScreenSize { width: 1512.0, height: 982.0 },
            scale: 2.0,
        }
    }

    pub fn with_screen(mut self, width: f32, height: f32, scale: f32) -> Self {
        self.screen = ScreenSize { width, height };
        self.scale = scale;
        self
    }

    pub fn view_count(&self) -> usize {
        self.views.lock().unwrap().len()
    }
}

impl Default for MacosPlatform {
    fn default() -> Self { Self::new() }
}

impl PlatformBridge for MacosPlatform {
    fn platform_id(&self) -> PlatformId {
        PlatformId::Macos
    }

    fn create_view(&self, view_type: ViewType, node_id: NodeId) -> NativeHandle {
        let mut h = self.next_handle.lock().unwrap();
        let handle = NativeHandle(*h);
        *h += 1;

        self.views.lock().unwrap().insert(handle.0, ViewRecord {
            view_type,
            node_id,
            children: Vec::new(),
        });

        handle
    }

    fn update_view(&self, handle: NativeHandle, _props: &PropsDiff) -> Result<(), PlatformError> {
        let views = self.views.lock().unwrap();
        if !views.contains_key(&handle.0) {
            return Err(PlatformError::ViewNotFound(handle));
        }
        // In production: forward props to NSView via FFI
        Ok(())
    }

    fn remove_view(&self, handle: NativeHandle) {
        self.views.lock().unwrap().remove(&handle.0);
    }

    fn insert_child(&self, parent: NativeHandle, child: NativeHandle, index: usize) {
        let mut views = self.views.lock().unwrap();
        if let Some(parent_record) = views.get_mut(&parent.0) {
            let idx = index.min(parent_record.children.len());
            parent_record.children.insert(idx, child);
        }
    }

    fn remove_child(&self, parent: NativeHandle, child: NativeHandle) {
        let mut views = self.views.lock().unwrap();
        if let Some(parent_record) = views.get_mut(&parent.0) {
            parent_record.children.retain(|c| *c != child);
        }
    }

    fn measure_text(&self, text: &str, style: &TextStyle, max_width: f32) -> TextMetrics {
        // Approximate measurement until connected to Core Text via FFI.
        // Uses system font metrics as baseline (SF Pro).
        let font_size = style.font_size.unwrap_or(13.0); // macOS default
        let char_width = font_size * 0.55; // SF Pro average
        let line_height = style.line_height.unwrap_or(font_size * 1.2);
        let total_width = char_width * text.len() as f32;

        let lines = if max_width > 0.0 && total_width > max_width {
            (total_width / max_width).ceil() as u32
        } else {
            1
        };

        TextMetrics {
            width: if lines > 1 { max_width } else { total_width },
            height: line_height * lines as f32,
            baseline: font_size * 0.8,
            line_count: lines,
        }
    }

    fn screen_size(&self) -> ScreenSize {
        self.screen
    }

    fn scale_factor(&self) -> f32 {
        self.scale
    }

    fn supports(&self, capability: PlatformCapability) -> bool {
        matches!(capability,
            PlatformCapability::MenuBar
            | PlatformCapability::SystemTray
            | PlatformCapability::MultiWindow
            | PlatformCapability::DragAndDrop
            | PlatformCapability::ContextMenu
            | PlatformCapability::NativeFilePicker
            | PlatformCapability::NativeDatePicker
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macos_create_and_remove() {
        let platform = MacosPlatform::new();
        assert_eq!(platform.platform_id(), PlatformId::Macos);

        let h1 = platform.create_view(ViewType::Container, NodeId(1));
        let h2 = platform.create_view(ViewType::Text, NodeId(2));
        assert_eq!(platform.view_count(), 2);

        platform.insert_child(h1, h2, 0);
        platform.remove_child(h1, h2);
        platform.remove_view(h2);
        assert_eq!(platform.view_count(), 1);
    }

    #[test]
    fn test_macos_capabilities() {
        let platform = MacosPlatform::new();
        assert!(platform.supports(PlatformCapability::MenuBar));
        assert!(platform.supports(PlatformCapability::MultiWindow));
        assert!(platform.supports(PlatformCapability::DragAndDrop));
        assert!(!platform.supports(PlatformCapability::Haptics));
        assert!(!platform.supports(PlatformCapability::Biometrics));
    }

    #[test]
    fn test_macos_screen_info() {
        let platform = MacosPlatform::new();
        let screen = platform.screen_size();
        assert_eq!(screen.width, 1512.0);
        assert_eq!(screen.height, 982.0);
        assert_eq!(platform.scale_factor(), 2.0);
    }

    #[test]
    fn test_macos_text_measurement() {
        let platform = MacosPlatform::new();
        let style = TextStyle { font_size: Some(16.0), ..TextStyle::default() };
        let metrics = platform.measure_text("Hello macOS", &style, 200.0);
        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
        assert_eq!(metrics.line_count, 1);
    }

    #[test]
    fn test_macos_update_missing_view() {
        let platform = MacosPlatform::new();
        let result = platform.update_view(NativeHandle(999), &PropsDiff::new());
        assert!(result.is_err());
    }
}
