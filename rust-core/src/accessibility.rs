//! Accessibility Layer — semantic roles, focus management, screen reader mapping.
//!
//! Every native widget framework already has accessibility built into its widgets.
//! Our job is to ensure the right semantics reach the platform:
//! - iOS: UIAccessibility (VoiceOver)
//! - Android: AccessibilityNodeInfo (TalkBack)
//! - macOS: NSAccessibility (VoiceOver)
//! - Windows: UI Automation (Narrator)
//! - Web: ARIA attributes
//!
//! The accessibility tree is a parallel structure to the shadow tree.
//! Not every visual node is an accessibility node — we merge/prune to match
//! how screen readers expect to navigate.

use crate::tree::NodeId;
use crate::platform::NativeHandle;
use std::collections::HashMap;

/// Semantic role of a UI element (maps to platform accessibility APIs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityRole {
    /// No semantic role — decorative or container (pruned from a11y tree).
    None,
    /// Interactive button.
    Button,
    /// Text content (heading, paragraph, label).
    Text,
    /// Heading (h1-h6 equivalent). Level 1-6.
    Heading,
    /// Text input field.
    TextField,
    /// Image with description.
    Image,
    /// Checkable toggle (switch, checkbox).
    Switch,
    /// Adjustable value (slider, stepper).
    Adjustable,
    /// Link (navigates somewhere).
    Link,
    /// Search field.
    SearchField,
    /// Tab bar or segmented control.
    TabBar,
    /// Individual tab.
    Tab,
    /// List (scrollable collection).
    List,
    /// List item.
    ListItem,
    /// Modal/dialog.
    Alert,
    /// Progress indicator.
    ProgressBar,
    /// Menu.
    Menu,
    /// Menu item.
    MenuItem,
}

/// Accessibility state for a node.
#[derive(Debug, Clone, Default)]
pub struct AccessibilityState {
    pub disabled: bool,
    pub selected: bool,
    pub checked: Option<bool>,  // None = not checkable, Some(true/false) = checkable
    pub expanded: Option<bool>, // None = not expandable
    pub busy: bool,
}

/// Accessibility value (for adjustable elements like sliders).
#[derive(Debug, Clone, Default)]
pub struct AccessibilityValue {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub now: Option<f64>,
    pub text: Option<String>,  // e.g., "50%" or "Medium"
}

/// Complete accessibility info for a node.
#[derive(Debug, Clone)]
pub struct AccessibilityInfo {
    pub role: AccessibilityRole,
    pub label: Option<String>,           // What screen reader announces
    pub hint: Option<String>,            // Usage hint ("double tap to activate")
    pub state: AccessibilityState,
    pub value: AccessibilityValue,
    pub heading_level: Option<u8>,       // 1-6 for Heading role
    pub live_region: LiveRegion,         // For dynamic content updates
    pub actions: Vec<AccessibilityAction>,
    pub is_modal: bool,                  // Traps focus within this subtree
    pub hides_descendants: bool,         // Children hidden from a11y tree
}

impl Default for AccessibilityInfo {
    fn default() -> Self {
        Self {
            role: AccessibilityRole::None,
            label: None,
            hint: None,
            state: AccessibilityState::default(),
            value: AccessibilityValue::default(),
            heading_level: None,
            live_region: LiveRegion::Off,
            actions: Vec::new(),
            is_modal: false,
            hides_descendants: false,
        }
    }
}

/// Live region announcement policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LiveRegion {
    /// No automatic announcements.
    #[default]
    Off,
    /// Announce changes when convenient (polite).
    Polite,
    /// Interrupt current announcement to announce changes.
    Assertive,
}

/// Custom accessibility actions.
#[derive(Debug, Clone)]
pub struct AccessibilityAction {
    pub name: String,
    pub label: String,  // Human-readable label for the action
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Focus management
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Manages keyboard/accessibility focus across the application.
pub struct FocusManager {
    /// Currently focused node.
    focused: Option<NodeId>,

    /// Focus order (tab index). Nodes not in this list use tree order.
    focus_order: Vec<NodeId>,

    /// Focus trap stack (for modals — focus stays within the top trap).
    focus_traps: Vec<NodeId>,

    /// Nodes that are focusable.
    focusable: HashMap<NodeId, FocusConfig>,
}

#[derive(Debug, Clone)]
pub struct FocusConfig {
    /// Tab index: -1 = not tabbable, 0 = natural order, >0 = explicit order.
    pub tab_index: i32,
    /// Whether this node should auto-focus when mounted.
    pub auto_focus: bool,
}

impl FocusManager {
    pub fn new() -> Self {
        Self {
            focused: None,
            focus_order: Vec::new(),
            focus_traps: Vec::new(),
            focusable: HashMap::new(),
        }
    }

    /// Register a node as focusable.
    pub fn register(&mut self, node_id: NodeId, config: FocusConfig) {
        if config.tab_index >= 0 {
            // Insert into focus order
            if config.tab_index > 0 {
                // Explicit order: insert at the right position
                let pos = self.focus_order.iter()
                    .position(|&id| {
                        self.focusable.get(&id)
                            .map(|c| c.tab_index > config.tab_index)
                            .unwrap_or(true)
                    })
                    .unwrap_or(self.focus_order.len());
                self.focus_order.insert(pos, node_id);
            } else {
                // Natural order (tab_index = 0): append
                self.focus_order.push(node_id);
            }
        }
        self.focusable.insert(node_id, config);
    }

    /// Unregister a node (when removed from tree).
    pub fn unregister(&mut self, node_id: NodeId) {
        self.focusable.remove(&node_id);
        self.focus_order.retain(|&id| id != node_id);
        if self.focused == Some(node_id) {
            self.focused = None;
        }
    }

    /// Move focus to a specific node.
    pub fn focus(&mut self, node_id: NodeId) -> Option<FocusChange> {
        let previous = self.focused;
        self.focused = Some(node_id);

        Some(FocusChange {
            previous,
            current: node_id,
        })
    }

    /// Move focus to the next focusable node (Tab key).
    pub fn focus_next(&mut self) -> Option<FocusChange> {
        let candidates = self.get_candidates();
        if candidates.is_empty() { return None; }

        let current_index = self.focused
            .and_then(|id| candidates.iter().position(|&c| c == id))
            .unwrap_or(candidates.len().wrapping_sub(1));

        let next_index = (current_index + 1) % candidates.len();
        self.focus(candidates[next_index])
    }

    /// Move focus to the previous focusable node (Shift+Tab).
    pub fn focus_previous(&mut self) -> Option<FocusChange> {
        let candidates = self.get_candidates();
        if candidates.is_empty() { return None; }

        let current_index = self.focused
            .and_then(|id| candidates.iter().position(|&c| c == id))
            .unwrap_or(0);

        let prev_index = if current_index == 0 {
            candidates.len() - 1
        } else {
            current_index - 1
        };

        self.focus(candidates[prev_index])
    }

    /// Push a focus trap (for modals). Focus cannot leave the trap subtree.
    pub fn push_trap(&mut self, root_node: NodeId) {
        self.focus_traps.push(root_node);
    }

    /// Pop a focus trap (when modal is dismissed).
    pub fn pop_trap(&mut self) -> Option<NodeId> {
        self.focus_traps.pop()
    }

    /// Get the currently focused node.
    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    /// Get focusable candidates, respecting the active focus trap.
    fn get_candidates(&self) -> Vec<NodeId> {
        if let Some(&_trap_root) = self.focus_traps.last() {
            // When a focus trap is active, only nodes within the trap are candidates.
            // TODO: filter by tree ancestry (requires tree reference).
            // For now, return all focusable nodes (trap filtering happens at dispatch).
            self.focus_order.clone()
        } else {
            self.focus_order.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct FocusChange {
    pub previous: Option<NodeId>,
    pub current: NodeId,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Platform accessibility bridge
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Platform bridges implement this to push accessibility info to the OS.
pub trait AccessibilityBridge {
    /// Update the accessibility info for a native view.
    fn update_accessibility(
        &self,
        handle: NativeHandle,
        info: &AccessibilityInfo,
    );

    /// Announce a message to the screen reader.
    fn announce(&self, message: &str, priority: LiveRegion);

    /// Move accessibility focus to a specific element.
    fn set_accessibility_focus(&self, handle: NativeHandle);
}

/// Maps our AccessibilityRole to platform-specific values.
/// Each platform bridge uses this in its update_accessibility implementation.
impl AccessibilityRole {
    /// iOS UIAccessibilityTraits mapping.
    pub fn ios_traits(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Button => "button",
            Self::Text => "staticText",
            Self::Heading => "header",
            Self::TextField => "none", // UITextField is inherently accessible
            Self::Image => "image",
            Self::Switch => "button", // + UISwitch is inherently accessible
            Self::Adjustable => "adjustable",
            Self::Link => "link",
            Self::SearchField => "searchField",
            Self::TabBar => "tabBar",
            Self::Tab => "button", // + selected state
            Self::List => "none", // UITableView handles this
            Self::ListItem => "none",
            Self::Alert => "none", // UIAlertController handles this
            Self::ProgressBar => "none", // UIProgressView handles this
            Self::Menu => "none",
            Self::MenuItem => "button",
        }
    }

    /// Web ARIA role mapping.
    pub fn aria_role(&self) -> &'static str {
        match self {
            Self::None => "presentation",
            Self::Button => "button",
            Self::Text => "",           // No role needed for text
            Self::Heading => "heading",
            Self::TextField => "textbox",
            Self::Image => "img",
            Self::Switch => "switch",
            Self::Adjustable => "slider",
            Self::Link => "link",
            Self::SearchField => "searchbox",
            Self::TabBar => "tablist",
            Self::Tab => "tab",
            Self::List => "list",
            Self::ListItem => "listitem",
            Self::Alert => "alert",
            Self::ProgressBar => "progressbar",
            Self::Menu => "menu",
            Self::MenuItem => "menuitem",
        }
    }

    /// Android AccessibilityNodeInfo className mapping.
    pub fn android_class(&self) -> &'static str {
        match self {
            Self::None => "android.view.View",
            Self::Button => "android.widget.Button",
            Self::Text => "android.widget.TextView",
            Self::Heading => "android.widget.TextView", // + heading flag
            Self::TextField => "android.widget.EditText",
            Self::Image => "android.widget.ImageView",
            Self::Switch => "android.widget.Switch",
            Self::Adjustable => "android.widget.SeekBar",
            Self::Link => "android.widget.TextView", // + clickable
            Self::SearchField => "android.widget.EditText",
            Self::TabBar => "android.widget.TabWidget",
            Self::Tab => "android.widget.TabWidget",
            Self::List => "android.widget.ListView",
            Self::ListItem => "android.widget.ListView",
            Self::Alert => "android.app.AlertDialog",
            Self::ProgressBar => "android.widget.ProgressBar",
            Self::Menu => "android.widget.PopupMenu",
            Self::MenuItem => "android.widget.PopupMenu",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_cycle() {
        let mut fm = FocusManager::new();
        fm.register(NodeId(1), FocusConfig { tab_index: 0, auto_focus: false });
        fm.register(NodeId(2), FocusConfig { tab_index: 0, auto_focus: false });
        fm.register(NodeId(3), FocusConfig { tab_index: 0, auto_focus: false });

        // Tab through all nodes
        let c = fm.focus_next().unwrap();
        assert_eq!(c.current, NodeId(1));

        let c = fm.focus_next().unwrap();
        assert_eq!(c.current, NodeId(2));

        let c = fm.focus_next().unwrap();
        assert_eq!(c.current, NodeId(3));

        // Wraps around
        let c = fm.focus_next().unwrap();
        assert_eq!(c.current, NodeId(1));
    }

    #[test]
    fn test_focus_reverse() {
        let mut fm = FocusManager::new();
        fm.register(NodeId(1), FocusConfig { tab_index: 0, auto_focus: false });
        fm.register(NodeId(2), FocusConfig { tab_index: 0, auto_focus: false });

        fm.focus(NodeId(2));
        let c = fm.focus_previous().unwrap();
        assert_eq!(c.current, NodeId(1));
    }

    #[test]
    fn test_role_mappings() {
        assert_eq!(AccessibilityRole::Button.aria_role(), "button");
        assert_eq!(AccessibilityRole::Switch.ios_traits(), "button");
        assert_eq!(AccessibilityRole::TextField.android_class(), "android.widget.EditText");
    }
}
