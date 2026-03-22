//! Android Mobile Platform Bridge
//!
//! Implements PlatformBridge for Android using Android Views concepts.
//! At this stage, this is a scaffold that records operations and will
//! later connect to the actual JNI bridge for Android View creation.
//!
//! Architecture:
//!   Rust PlatformBridge → JNI → Kotlin/Java Android Views wrapper
//!
//! The Android bridge handles ViewGroup hierarchy, Paint/Canvas text
//! measurement, and Android-specific capabilities like Haptics,
//! Biometrics, and BackgroundFetch.

use crate::platform::*;
use crate::tree::NodeId;
use std::collections::HashMap;
use std::sync::Mutex;

/// Android platform bridge.
///
/// In production, each method will call through JNI to the Kotlin/Java layer.
/// For now, we maintain an in-memory view registry for testing and development.
pub struct AndroidPlatform {
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

impl AndroidPlatform {
    pub fn new() -> Self {
        Self {
            next_handle: Mutex::new(1),
            views: Mutex::new(HashMap::new()),
            // Default: Pixel 6 logical resolution (dp)
            screen: ScreenSize {
                width: 412.0,
                height: 732.0,
            },
            scale: 2.625, // xxhdpi
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

impl Default for AndroidPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBridge for AndroidPlatform {
    fn platform_id(&self) -> PlatformId {
        PlatformId::Android
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
        // In production: forward props to Android View via JNI
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
        // Approximate measurement until connected to Android Paint.measureText via JNI.
        // Uses Roboto metrics as baseline (Android system font).
        let font_size = style.font_size.unwrap_or(14.0); // Android default (14sp)
        let char_width = font_size * 0.54; // Roboto average
        let line_height = style.line_height.unwrap_or(font_size * 1.25);
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
            PlatformCapability::Haptics
                | PlatformCapability::Biometrics
                | PlatformCapability::PushNotifications
                | PlatformCapability::BackgroundFetch
                | PlatformCapability::NativeFilePicker
                | PlatformCapability::NativeDatePicker
                | PlatformCapability::ContextMenu
                | PlatformCapability::NativeShare
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_android_create_and_remove() {
        let platform = AndroidPlatform::new();
        assert_eq!(platform.platform_id(), PlatformId::Android);

        let h1 = platform.create_view(ViewType::Container, NodeId(1));
        let h2 = platform.create_view(ViewType::Button, NodeId(2));
        assert_eq!(platform.view_count(), 2);

        platform.insert_child(h1, h2, 0);
        platform.remove_child(h1, h2);
        platform.remove_view(h2);
        assert_eq!(platform.view_count(), 1);
    }

    #[test]
    fn test_android_capabilities() {
        let platform = AndroidPlatform::new();
        assert!(platform.supports(PlatformCapability::Haptics));
        assert!(platform.supports(PlatformCapability::Biometrics));
        assert!(platform.supports(PlatformCapability::PushNotifications));
        assert!(platform.supports(PlatformCapability::BackgroundFetch));
        assert!(platform.supports(PlatformCapability::NativeShare));
        assert!(!platform.supports(PlatformCapability::MenuBar));
        assert!(!platform.supports(PlatformCapability::SystemTray));
        assert!(!platform.supports(PlatformCapability::MultiWindow));
    }

    #[test]
    fn test_android_screen_info() {
        let platform = AndroidPlatform::new();
        let screen = platform.screen_size();
        assert_eq!(screen.width, 412.0);
        assert_eq!(screen.height, 732.0);
        assert_eq!(platform.scale_factor(), 2.625);
    }

    #[test]
    fn test_android_custom_screen() {
        // Samsung Galaxy Tab S9 Ultra
        let platform = AndroidPlatform::new().with_screen(900.0, 1422.0, 1.5);
        let screen = platform.screen_size();
        assert_eq!(screen.width, 900.0);
        assert_eq!(screen.height, 1422.0);
        assert_eq!(platform.scale_factor(), 1.5);
    }

    #[test]
    fn test_android_text_measurement() {
        let platform = AndroidPlatform::new();
        let style = TextStyle {
            font_size: Some(16.0),
            ..TextStyle::default()
        };
        let metrics = platform.measure_text("Hello Android", &style, 200.0);
        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
        assert_eq!(metrics.line_count, 1);
    }

    #[test]
    fn test_android_text_wrapping() {
        let platform = AndroidPlatform::new();
        let style = TextStyle {
            font_size: Some(16.0),
            ..TextStyle::default()
        };
        let metrics = platform.measure_text("This is a longer text that should wrap", &style, 50.0);
        assert!(metrics.line_count > 1);
        assert_eq!(metrics.width, 50.0);
    }

    #[test]
    fn test_android_update_missing_view() {
        let platform = AndroidPlatform::new();
        let result = platform.update_view(NativeHandle(999), &PropsDiff::new());
        assert!(result.is_err());
    }
}
