//! Web Platform Bridge
//!
//! Implements PlatformBridge for the Web using DOM/CSS concepts.
//! At this stage, this is a scaffold that records operations and will
//! later connect to the browser DOM via `wasm-bindgen` bindings.
//!
//! Architecture:
//!   Rust PlatformBridge → wasm-bindgen → JavaScript DOM API wrapper
//!
//! The Web bridge handles DOM element hierarchy, Canvas-based text
//! measurement, and Web-specific capabilities like DragAndDrop and
//! NativeShare (Web Share API).

use crate::platform::*;
use crate::tree::NodeId;
use std::sync::Mutex;
use std::collections::HashMap;

/// Web platform bridge.
///
/// In production, each method will call through `wasm-bindgen` to the browser DOM.
/// For now, we maintain an in-memory view registry for testing and development.
pub struct WebPlatform {
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

impl WebPlatform {
    pub fn new() -> Self {
        Self {
            next_handle: Mutex::new(1),
            views: Mutex::new(HashMap::new()),
            // Default: common 1080p browser viewport
            screen: ScreenSize { width: 1920.0, height: 1080.0 },
            scale: 1.0,
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

impl Default for WebPlatform {
    fn default() -> Self { Self::new() }
}

impl PlatformBridge for WebPlatform {
    fn platform_id(&self) -> PlatformId {
        PlatformId::Web
    }

    fn create_view(&self, view_type: ViewType, node_id: NodeId) -> NativeHandle {
        let mut h = self.next_handle.lock().unwrap();
        let handle = NativeHandle(*h);
        *h += 1;

        self.views.lock().unwrap().insert(handle.0, ViewRecord {
            _view_type: view_type,
            _node_id: node_id,
            children: Vec::new(),
        });

        handle
    }

    fn update_view(&self, handle: NativeHandle, _props: &PropsDiff) -> Result<(), PlatformError> {
        let views = self.views.lock().unwrap();
        if !views.contains_key(&handle.0) {
            return Err(PlatformError::ViewNotFound(handle));
        }
        // In production: forward props to DOM element via wasm-bindgen
        // e.g., element.style.setProperty(), element.setAttribute(), etc.
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
        // Approximate measurement until connected to Canvas.measureText via wasm-bindgen.
        // Uses system-ui / sans-serif metrics as baseline (browser default).
        let font_size = style.font_size.unwrap_or(16.0); // Browser default
        let char_width = font_size * 0.53; // system-ui average
        let line_height = style.line_height.unwrap_or(font_size * 1.15);
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
            PlatformCapability::DragAndDrop
            | PlatformCapability::NativeShare       // Web Share API
            | PlatformCapability::ContextMenu
            | PlatformCapability::NativeFilePicker  // <input type="file">
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_create_and_remove() {
        let platform = WebPlatform::new();
        assert_eq!(platform.platform_id(), PlatformId::Web);

        let h1 = platform.create_view(ViewType::Container, NodeId(1));
        let h2 = platform.create_view(ViewType::Text, NodeId(2));
        let h3 = platform.create_view(ViewType::Image, NodeId(3));
        assert_eq!(platform.view_count(), 3);

        platform.insert_child(h1, h2, 0);
        platform.insert_child(h1, h3, 1);
        platform.remove_child(h1, h2);
        platform.remove_view(h2);
        assert_eq!(platform.view_count(), 2);
    }

    #[test]
    fn test_web_capabilities() {
        let platform = WebPlatform::new();
        assert!(platform.supports(PlatformCapability::DragAndDrop));
        assert!(platform.supports(PlatformCapability::NativeShare));
        assert!(platform.supports(PlatformCapability::ContextMenu));
        assert!(platform.supports(PlatformCapability::NativeFilePicker));
        assert!(!platform.supports(PlatformCapability::Haptics));
        assert!(!platform.supports(PlatformCapability::Biometrics));
        assert!(!platform.supports(PlatformCapability::MenuBar));
        assert!(!platform.supports(PlatformCapability::SystemTray));
        assert!(!platform.supports(PlatformCapability::PushNotifications));
    }

    #[test]
    fn test_web_screen_info() {
        let platform = WebPlatform::new();
        let screen = platform.screen_size();
        assert_eq!(screen.width, 1920.0);
        assert_eq!(screen.height, 1080.0);
        assert_eq!(platform.scale_factor(), 1.0);
    }

    #[test]
    fn test_web_custom_screen() {
        // Retina MacBook in browser
        let platform = WebPlatform::new().with_screen(1440.0, 900.0, 2.0);
        let screen = platform.screen_size();
        assert_eq!(screen.width, 1440.0);
        assert_eq!(screen.height, 900.0);
        assert_eq!(platform.scale_factor(), 2.0);
    }

    #[test]
    fn test_web_mobile_viewport() {
        // Mobile Safari on iPhone
        let platform = WebPlatform::new().with_screen(375.0, 667.0, 2.0);
        let screen = platform.screen_size();
        assert_eq!(screen.width, 375.0);
        assert_eq!(screen.height, 667.0);
        assert_eq!(platform.scale_factor(), 2.0);
    }

    #[test]
    fn test_web_text_measurement() {
        let platform = WebPlatform::new();
        let style = TextStyle { font_size: Some(16.0), ..TextStyle::default() };
        let metrics = platform.measure_text("Hello Web", &style, 200.0);
        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
        assert_eq!(metrics.line_count, 1);
    }

    #[test]
    fn test_web_text_wrapping() {
        let platform = WebPlatform::new();
        let style = TextStyle { font_size: Some(16.0), ..TextStyle::default() };
        let metrics = platform.measure_text("This is a longer text that should wrap in browser", &style, 50.0);
        assert!(metrics.line_count > 1);
        assert_eq!(metrics.width, 50.0);
    }

    #[test]
    fn test_web_update_missing_view() {
        let platform = WebPlatform::new();
        let result = platform.update_view(NativeHandle(999), &PropsDiff::new());
        assert!(result.is_err());
    }
}
