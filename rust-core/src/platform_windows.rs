//! Windows Desktop Platform Bridge
//!
//! Implements PlatformBridge for Windows using WinUI 3 / Win32 concepts.
//! At this stage, this is a scaffold that records operations and will
//! later connect to the actual C++/WinRT bridge via FFI.
//!
//! Architecture:
//!   Rust PlatformBridge → FFI (C ABI) → C++/WinRT WinUI wrapper
//!
//! The Windows bridge handles XAML-like view hierarchy, DirectWrite
//! text measurement, and Windows-specific capabilities.

use crate::platform::*;
use crate::tree::NodeId;
use std::collections::HashMap;
use std::sync::Mutex;

/// Windows platform bridge.
///
/// In production, each method will call through FFI to the C++/WinRT layer.
/// For now, we maintain an in-memory view registry for testing and development.
pub struct WindowsPlatform {
    next_handle: Mutex<u64>,
    views: Mutex<HashMap<u64, ViewRecord>>,
    screen: ScreenSize,
    scale: f32,
}

struct ViewRecord {
    _view_type: ViewType,
    _node_id: NodeId,
    children: Vec<NativeHandle>,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            next_handle: Mutex::new(1),
            views: Mutex::new(HashMap::new()),
            // Default: 1080p logical resolution at 150% scaling
            screen: ScreenSize {
                width: 1280.0,
                height: 720.0,
            },
            scale: 1.5,
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

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBridge for WindowsPlatform {
    fn platform_id(&self) -> PlatformId {
        PlatformId::Windows
    }

    fn create_view(&self, view_type: ViewType, node_id: NodeId) -> NativeHandle {
        let mut h = self.next_handle.lock().unwrap();
        let handle = NativeHandle(*h);
        *h += 1;

        self.views.lock().unwrap().insert(
            handle.0,
            ViewRecord {
                _view_type: view_type,
                _node_id: node_id,
                children: Vec::new(),
            },
        );

        handle
    }

    fn update_view(&self, handle: NativeHandle, _props: &PropsDiff) -> Result<(), PlatformError> {
        let views = self.views.lock().unwrap();
        if !views.contains_key(&handle.0) {
            return Err(PlatformError::ViewNotFound(handle));
        }
        // In production: forward props to WinUI XAML control via FFI
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
        // Approximate measurement until connected to DirectWrite via FFI.
        // Uses Segoe UI metrics as baseline (Windows system font).
        let font_size = style.font_size.unwrap_or(14.0); // Windows default
        let char_width = font_size * 0.52; // Segoe UI average
        let line_height = style.line_height.unwrap_or(font_size * 1.33);
        let total_width = char_width * text.len() as f32;

        let lines = if max_width > 0.0 && total_width > max_width {
            (total_width / max_width).ceil() as u32
        } else {
            1
        };

        TextMetrics {
            width: if lines > 1 { max_width } else { total_width },
            height: line_height * lines as f32,
            baseline: font_size * 0.78,
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
        matches!(
            capability,
            PlatformCapability::SystemTray
                | PlatformCapability::MultiWindow
                | PlatformCapability::DragAndDrop
                | PlatformCapability::ContextMenu
                | PlatformCapability::NativeFilePicker
                | PlatformCapability::NativeDatePicker
                | PlatformCapability::PushNotifications
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_create_and_remove() {
        let platform = WindowsPlatform::new();
        assert_eq!(platform.platform_id(), PlatformId::Windows);

        let h1 = platform.create_view(ViewType::Container, NodeId(1));
        let h2 = platform.create_view(ViewType::Button, NodeId(2));
        assert_eq!(platform.view_count(), 2);

        platform.insert_child(h1, h2, 0);
        platform.remove_child(h1, h2);
        platform.remove_view(h2);
        assert_eq!(platform.view_count(), 1);
    }

    #[test]
    fn test_windows_capabilities() {
        let platform = WindowsPlatform::new();
        assert!(platform.supports(PlatformCapability::SystemTray));
        assert!(platform.supports(PlatformCapability::MultiWindow));
        assert!(platform.supports(PlatformCapability::DragAndDrop));
        assert!(platform.supports(PlatformCapability::PushNotifications));
        assert!(!platform.supports(PlatformCapability::MenuBar)); // Windows doesn't have macOS-style menu bar
        assert!(!platform.supports(PlatformCapability::Haptics));
    }

    #[test]
    fn test_windows_screen_info() {
        let platform = WindowsPlatform::new();
        let screen = platform.screen_size();
        assert_eq!(screen.width, 1280.0);
        assert_eq!(screen.height, 720.0);
        assert_eq!(platform.scale_factor(), 1.5);
    }

    #[test]
    fn test_windows_custom_screen() {
        let platform = WindowsPlatform::new().with_screen(2560.0, 1440.0, 2.0);
        let screen = platform.screen_size();
        assert_eq!(screen.width, 2560.0);
        assert_eq!(platform.scale_factor(), 2.0);
    }

    #[test]
    fn test_windows_text_measurement() {
        let platform = WindowsPlatform::new();
        let style = TextStyle {
            font_size: Some(16.0),
            ..TextStyle::default()
        };
        let metrics = platform.measure_text("Hello Windows", &style, 200.0);
        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
        assert_eq!(metrics.line_count, 1);
    }

    #[test]
    fn test_windows_update_missing_view() {
        let platform = WindowsPlatform::new();
        let result = platform.update_view(NativeHandle(999), &PropsDiff::new());
        assert!(result.is_err());
    }
}
