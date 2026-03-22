//! Component Library — Built-in UI Primitives
//!
//! Defines the core set of React-like components that AppScale supports
//! out of the box. Each component descriptor maps a component name to:
//! - a `ViewType` (what native view to create)
//! - default props
//! - default layout style
//! - supported children mode (leaf vs container)
//!
//! The host-config (TypeScript side) uses component names like "View", "Text",
//! "Image", etc. The engine resolves them here to determine how to create and
//! configure the native view.

use crate::platform::{ViewType, PropValue, PropsDiff};
use crate::layout::LayoutStyle;
use std::collections::HashMap;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component Descriptor
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// How a component handles children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildrenMode {
    /// Accepts child components (e.g., View, ScrollView).
    Container,
    /// Leaf node — no children allowed (e.g., Image, ActivityIndicator).
    Leaf,
    /// Accepts only text children (e.g., Text, TextInput).
    TextOnly,
}

/// Describes a built-in component.
#[derive(Debug, Clone)]
pub struct ComponentDescriptor {
    pub name: &'static str,
    pub view_type: ViewType,
    pub children_mode: ChildrenMode,
    pub default_props: PropsDiff,
    pub default_style: LayoutStyle,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component Registry
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Registry of all built-in components.
pub struct ComponentRegistry {
    components: HashMap<&'static str, ComponentDescriptor>,
}

impl ComponentRegistry {
    /// Create a registry populated with all built-in components.
    pub fn new() -> Self {
        let mut registry = Self {
            components: HashMap::new(),
        };
        registry.register_builtins();
        registry
    }

    /// Look up a component by name.
    pub fn get(&self, name: &str) -> Option<&ComponentDescriptor> {
        self.components.get(name)
    }

    /// Resolve a component name to a ViewType.
    pub fn resolve_view_type(&self, name: &str) -> Option<ViewType> {
        self.components.get(name).map(|d| d.view_type.clone())
    }

    /// List all registered component names.
    pub fn component_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.components.keys().copied().collect();
        names.sort();
        names
    }

    /// Number of registered components.
    pub fn count(&self) -> usize {
        self.components.len()
    }

    fn register(&mut self, descriptor: ComponentDescriptor) {
        self.components.insert(descriptor.name, descriptor);
    }

    fn register_builtins(&mut self) {
        // ──── Core Primitives ────
        self.register(view_component());
        self.register(text_component());
        self.register(image_component());
        self.register(scroll_view_component());
        self.register(text_input_component());

        // ──── Lists ────
        self.register(flat_list_component());
        self.register(section_list_component());

        // ──── Navigation ────
        self.register(stack_navigator_component());
        self.register(tab_navigator_component());
        self.register(drawer_navigator_component());

        // ──── Form Controls ────
        self.register(switch_component());
        self.register(slider_component());
        self.register(picker_component());
        self.register(date_picker_component());

        // ──── Feedback ────
        self.register(button_component());
        self.register(pressable_component());
        self.register(touchable_opacity_component());
        self.register(activity_indicator_component());

        // ──── Layout ────
        self.register(safe_area_view_component());
        self.register(keyboard_avoiding_view_component());
        self.register(modal_component());

        // ──── Media ────
        self.register(video_component());
        self.register(camera_component());
        self.register(web_view_component());
    }
}

impl Default for ComponentRegistry {
    fn default() -> Self { Self::new() }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core Primitives
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn view_component() -> ComponentDescriptor {
    ComponentDescriptor {
        name: "View",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: PropsDiff::new(),
        default_style: LayoutStyle::default(),
    }
}

fn text_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("numberOfLines", PropValue::I32(0)); // 0 = unlimited
    props.set("selectable", PropValue::Bool(false));

    ComponentDescriptor {
        name: "Text",
        view_type: ViewType::Text,
        children_mode: ChildrenMode::TextOnly,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn image_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("resizeMode", PropValue::String("cover".into()));

    ComponentDescriptor {
        name: "Image",
        view_type: ViewType::Image,
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn scroll_view_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("horizontal", PropValue::Bool(false));
    props.set("showsScrollIndicator", PropValue::Bool(true));
    props.set("bounces", PropValue::Bool(true));
    props.set("pagingEnabled", PropValue::Bool(false));

    ComponentDescriptor {
        name: "ScrollView",
        view_type: ViewType::ScrollView,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn text_input_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("editable", PropValue::Bool(true));
    props.set("multiline", PropValue::Bool(false));
    props.set("secureTextEntry", PropValue::Bool(false));
    props.set("autoCapitalize", PropValue::String("sentences".into()));
    props.set("autoCorrect", PropValue::Bool(true));
    props.set("placeholder", PropValue::String(String::new()));

    ComponentDescriptor {
        name: "TextInput",
        view_type: ViewType::TextInput,
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Lists
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn flat_list_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("horizontal", PropValue::Bool(false));
    props.set("initialNumToRender", PropValue::I32(10));
    props.set("windowSize", PropValue::I32(21)); // items above/below viewport
    props.set("removeClippedSubviews", PropValue::Bool(true));

    ComponentDescriptor {
        name: "FlatList",
        view_type: ViewType::ScrollView, // Backed by ScrollView with recycling
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn section_list_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("stickySectionHeaders", PropValue::Bool(true));
    props.set("initialNumToRender", PropValue::I32(10));

    ComponentDescriptor {
        name: "SectionList",
        view_type: ViewType::ScrollView,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Navigation Components
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn stack_navigator_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("headerShown", PropValue::Bool(true));
    props.set("gestureEnabled", PropValue::Bool(true));
    props.set("animationEnabled", PropValue::Bool(true));

    ComponentDescriptor {
        name: "StackNavigator",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn tab_navigator_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("tabBarPosition", PropValue::String("bottom".into()));
    props.set("lazy", PropValue::Bool(true));

    ComponentDescriptor {
        name: "TabNavigator",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn drawer_navigator_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("drawerPosition", PropValue::String("left".into()));
    props.set("drawerType", PropValue::String("front".into()));
    props.set("swipeEnabled", PropValue::Bool(true));

    ComponentDescriptor {
        name: "DrawerNavigator",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Form Controls
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn switch_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("value", PropValue::Bool(false));
    props.set("disabled", PropValue::Bool(false));

    ComponentDescriptor {
        name: "Switch",
        view_type: ViewType::Switch,
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn slider_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("minimumValue", PropValue::F32(0.0));
    props.set("maximumValue", PropValue::F32(1.0));
    props.set("step", PropValue::F32(0.0)); // continuous
    props.set("value", PropValue::F32(0.0));
    props.set("disabled", PropValue::Bool(false));

    ComponentDescriptor {
        name: "Slider",
        view_type: ViewType::Slider,
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn picker_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("enabled", PropValue::Bool(true));

    ComponentDescriptor {
        name: "Picker",
        view_type: ViewType::Custom("Picker".into()),
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn date_picker_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("mode", PropValue::String("date".into())); // date, time, datetime

    ComponentDescriptor {
        name: "DatePicker",
        view_type: ViewType::DatePicker,
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Feedback Components
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn button_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("disabled", PropValue::Bool(false));

    ComponentDescriptor {
        name: "Button",
        view_type: ViewType::Button,
        children_mode: ChildrenMode::TextOnly,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn pressable_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("disabled", PropValue::Bool(false));

    ComponentDescriptor {
        name: "Pressable",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn touchable_opacity_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("activeOpacity", PropValue::F32(0.2));
    props.set("disabled", PropValue::Bool(false));

    ComponentDescriptor {
        name: "TouchableOpacity",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn activity_indicator_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("animating", PropValue::Bool(true));
    props.set("size", PropValue::String("small".into()));

    ComponentDescriptor {
        name: "ActivityIndicator",
        view_type: ViewType::ActivityIndicator,
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Layout Components
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn safe_area_view_component() -> ComponentDescriptor {
    ComponentDescriptor {
        name: "SafeAreaView",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: PropsDiff::new(),
        default_style: LayoutStyle::default(),
    }
}

fn keyboard_avoiding_view_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("behavior", PropValue::String("padding".into())); // padding, height, position
    props.set("enabled", PropValue::Bool(true));

    ComponentDescriptor {
        name: "KeyboardAvoidingView",
        view_type: ViewType::Container,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn modal_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("visible", PropValue::Bool(false));
    props.set("animationType", PropValue::String("none".into())); // none, slide, fade
    props.set("transparent", PropValue::Bool(false));

    ComponentDescriptor {
        name: "Modal",
        view_type: ViewType::Modal,
        children_mode: ChildrenMode::Container,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Media Components
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn video_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("paused", PropValue::Bool(true));
    props.set("muted", PropValue::Bool(false));
    props.set("repeat", PropValue::Bool(false));
    props.set("resizeMode", PropValue::String("contain".into()));
    props.set("controls", PropValue::Bool(true));

    ComponentDescriptor {
        name: "Video",
        view_type: ViewType::Custom("Video".into()),
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn camera_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("facing", PropValue::String("back".into())); // front, back
    props.set("flashMode", PropValue::String("off".into())); // on, off, auto

    ComponentDescriptor {
        name: "Camera",
        view_type: ViewType::Custom("Camera".into()),
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

fn web_view_component() -> ComponentDescriptor {
    let mut props = PropsDiff::new();
    props.set("javaScriptEnabled", PropValue::Bool(true));
    props.set("domStorageEnabled", PropValue::Bool(true));
    props.set("scalesPageToFit", PropValue::Bool(true));

    ComponentDescriptor {
        name: "WebView",
        view_type: ViewType::Custom("WebView".into()),
        children_mode: ChildrenMode::Leaf,
        default_props: props,
        default_style: LayoutStyle::default(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_all_components() {
        let registry = ComponentRegistry::new();
        // 5 core + 2 lists + 3 nav + 4 form + 4 feedback + 3 layout + 3 media = 24
        assert_eq!(registry.count(), 24);
    }

    #[test]
    fn test_core_primitives_registered() {
        let registry = ComponentRegistry::new();
        assert!(registry.get("View").is_some());
        assert!(registry.get("Text").is_some());
        assert!(registry.get("Image").is_some());
        assert!(registry.get("ScrollView").is_some());
        assert!(registry.get("TextInput").is_some());
    }

    #[test]
    fn test_view_type_mapping() {
        let registry = ComponentRegistry::new();
        assert_eq!(registry.resolve_view_type("View"), Some(ViewType::Container));
        assert_eq!(registry.resolve_view_type("Text"), Some(ViewType::Text));
        assert_eq!(registry.resolve_view_type("Image"), Some(ViewType::Image));
        assert_eq!(registry.resolve_view_type("Button"), Some(ViewType::Button));
        assert_eq!(registry.resolve_view_type("Switch"), Some(ViewType::Switch));
        assert_eq!(registry.resolve_view_type("Modal"), Some(ViewType::Modal));
    }

    #[test]
    fn test_children_mode() {
        let registry = ComponentRegistry::new();
        assert_eq!(registry.get("View").unwrap().children_mode, ChildrenMode::Container);
        assert_eq!(registry.get("Text").unwrap().children_mode, ChildrenMode::TextOnly);
        assert_eq!(registry.get("Image").unwrap().children_mode, ChildrenMode::Leaf);
        assert_eq!(registry.get("ActivityIndicator").unwrap().children_mode, ChildrenMode::Leaf);
        assert_eq!(registry.get("ScrollView").unwrap().children_mode, ChildrenMode::Container);
    }

    #[test]
    fn test_default_props() {
        let registry = ComponentRegistry::new();

        let text_input = registry.get("TextInput").unwrap();
        assert!(!text_input.default_props.is_empty());

        let view = registry.get("View").unwrap();
        assert!(view.default_props.is_empty());
    }

    #[test]
    fn test_component_names_sorted() {
        let registry = ComponentRegistry::new();
        let names = registry.component_names();
        assert!(names.len() == 24);
        // Verify sorted
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_unknown_component() {
        let registry = ComponentRegistry::new();
        assert!(registry.get("NonExistent").is_none());
        assert_eq!(registry.resolve_view_type("NonExistent"), None);
    }

    #[test]
    fn test_list_components() {
        let registry = ComponentRegistry::new();
        let flat_list = registry.get("FlatList").unwrap();
        assert_eq!(flat_list.view_type, ViewType::ScrollView);
        assert_eq!(flat_list.children_mode, ChildrenMode::Container);

        let section_list = registry.get("SectionList").unwrap();
        assert_eq!(section_list.view_type, ViewType::ScrollView);
    }

    #[test]
    fn test_navigation_components() {
        let registry = ComponentRegistry::new();
        assert!(registry.get("StackNavigator").is_some());
        assert!(registry.get("TabNavigator").is_some());
        assert!(registry.get("DrawerNavigator").is_some());
    }

    #[test]
    fn test_media_components() {
        let registry = ComponentRegistry::new();
        let video = registry.get("Video").unwrap();
        assert_eq!(video.view_type, ViewType::Custom("Video".into()));
        assert_eq!(video.children_mode, ChildrenMode::Leaf);

        let camera = registry.get("Camera").unwrap();
        assert_eq!(camera.view_type, ViewType::Custom("Camera".into()));

        let webview = registry.get("WebView").unwrap();
        assert_eq!(webview.view_type, ViewType::Custom("WebView".into()));
    }
}
