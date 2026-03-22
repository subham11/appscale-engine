//! Navigation System — stack, tab, modal, and deep linking.
//!
//! Navigation is state + stack + transitions.
//! The Rust side manages the navigation state machine.
//! The platform bridge handles native transition animations.
//!
//! Design: Navigation state lives in Rust (not React state) because:
//! 1. Platform bridges need it for native transitions (UINavigationController, etc.)
//! 2. Deep links must resolve before React renders
//! 3. Back button/gesture handling is synchronous and platform-specific

use crate::tree::NodeId;
use crate::platform::{PlatformBridge, NativeHandle};
use std::collections::HashMap;

/// A route definition (registered at app startup).
#[derive(Debug, Clone)]
pub struct RouteDefinition {
    pub name: String,
    pub path: Option<String>,           // URL path for deep linking, e.g. "/profile/:id"
    pub presentation: Presentation,
    pub options: RouteOptions,
}

/// How a screen is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    /// Push onto the stack (slide from right on iOS, slide up on Android)
    Push,
    /// Present as a modal (slide up on iOS, dialog on Android)
    Modal,
    /// Replace current screen (no animation, or custom)
    Replace,
    /// Tab screen (managed by tab bar)
    Tab,
}

#[derive(Debug, Clone, Default)]
pub struct RouteOptions {
    pub title: Option<String>,
    pub header_shown: bool,
    pub gesture_enabled: bool,
    pub animation: TransitionAnimation,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TransitionAnimation {
    #[default]
    Platform,       // Use native platform animation
    SlideRight,
    SlideUp,
    Fade,
    None,
}

/// An active screen in the navigation stack.
#[derive(Debug, Clone)]
pub struct Screen {
    pub id: ScreenId,
    pub route_name: String,
    pub params: HashMap<String, String>,
    pub presentation: Presentation,
    pub root_node: Option<NodeId>,       // Root shadow tree node for this screen
    pub native_handle: Option<NativeHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenId(pub u64);

/// Navigation action (sent from React or deep link resolver).
#[derive(Debug, Clone)]
pub enum NavigationAction {
    Push { route: String, params: HashMap<String, String> },
    Pop,
    PopToRoot,
    Replace { route: String, params: HashMap<String, String> },
    PresentModal { route: String, params: HashMap<String, String> },
    DismissModal,
    SwitchTab { index: usize },
    DeepLink { url: String },
    GoBack,     // Platform back button (Android, Windows, web browser)
}

/// Navigation event (sent to React for rendering decisions).
#[derive(Debug, Clone)]
pub enum NavigationEvent {
    /// A new screen should be rendered.
    ScreenMounted { screen_id: ScreenId, route_name: String, params: HashMap<String, String> },
    /// A screen is being removed (animate out, then destroy).
    ScreenUnmounting { screen_id: ScreenId },
    /// The active screen changed (for tab bar highlighting, etc.).
    ActiveScreenChanged { screen_id: ScreenId },
    /// Navigation state changed (for DevTools).
    StateChanged { stack_depth: usize, active_route: String },
}

/// The navigator manages all navigation state.
pub struct Navigator {
    /// Route definitions (registered at startup).
    routes: HashMap<String, RouteDefinition>,

    /// The main stack.
    stack: Vec<Screen>,

    /// Modal stack (layered on top of main stack).
    modals: Vec<Screen>,

    /// Tab screens (if using tab navigation).
    tabs: Vec<Screen>,
    active_tab: usize,

    /// Screen ID counter.
    next_screen_id: u64,

    /// Pending events to deliver to React.
    pending_events: Vec<NavigationEvent>,
}

impl Navigator {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            stack: Vec::new(),
            modals: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
            next_screen_id: 1,
            pending_events: Vec::new(),
        }
    }

    /// Register a route definition.
    pub fn register_route(&mut self, route: RouteDefinition) {
        self.routes.insert(route.name.clone(), route);
    }

    /// Process a navigation action. Returns events for React to handle.
    pub fn dispatch(&mut self, action: NavigationAction) -> Vec<NavigationEvent> {
        self.pending_events.clear();

        match action {
            NavigationAction::Push { route, params } => {
                self.push_screen(&route, params, Presentation::Push);
            }
            NavigationAction::Pop => {
                self.pop_screen();
            }
            NavigationAction::PopToRoot => {
                while self.stack.len() > 1 {
                    self.pop_screen();
                }
            }
            NavigationAction::Replace { route, params } => {
                // Pop current, push new (no animation)
                if !self.stack.is_empty() {
                    let screen_id = self.stack.last().unwrap().id;
                    self.pending_events.push(NavigationEvent::ScreenUnmounting { screen_id });
                    self.stack.pop();
                }
                self.push_screen(&route, params, Presentation::Replace);
            }
            NavigationAction::PresentModal { route, params } => {
                self.push_screen(&route, params, Presentation::Modal);
            }
            NavigationAction::DismissModal => {
                if let Some(modal) = self.modals.pop() {
                    self.pending_events.push(NavigationEvent::ScreenUnmounting {
                        screen_id: modal.id,
                    });
                    self.emit_active_changed();
                }
            }
            NavigationAction::SwitchTab { index } => {
                if index < self.tabs.len() {
                    self.active_tab = index;
                    self.pending_events.push(NavigationEvent::ActiveScreenChanged {
                        screen_id: self.tabs[index].id,
                    });
                }
            }
            NavigationAction::DeepLink { url } => {
                self.resolve_deep_link(&url);
            }
            NavigationAction::GoBack => {
                // Priority: dismiss modal → pop stack → do nothing
                if !self.modals.is_empty() {
                    self.dispatch(NavigationAction::DismissModal);
                } else if self.stack.len() > 1 {
                    self.pop_screen();
                }
                // If stack has only 1 screen, GoBack is a no-op
                // (platform bridge handles app minimize/exit)
            }
        }

        std::mem::take(&mut self.pending_events)
    }

    fn push_screen(
        &mut self,
        route_name: &str,
        params: HashMap<String, String>,
        presentation: Presentation,
    ) {
        let screen_id = ScreenId(self.next_screen_id);
        self.next_screen_id += 1;

        let screen = Screen {
            id: screen_id,
            route_name: route_name.to_string(),
            params: params.clone(),
            presentation,
            root_node: None,
            native_handle: None,
        };

        match presentation {
            Presentation::Modal => self.modals.push(screen),
            Presentation::Tab => self.tabs.push(screen),
            _ => self.stack.push(screen),
        }

        self.pending_events.push(NavigationEvent::ScreenMounted {
            screen_id,
            route_name: route_name.to_string(),
            params,
        });

        self.emit_active_changed();
    }

    fn pop_screen(&mut self) {
        if self.stack.len() <= 1 {
            return; // Never pop the root screen
        }

        if let Some(screen) = self.stack.pop() {
            self.pending_events.push(NavigationEvent::ScreenUnmounting {
                screen_id: screen.id,
            });
            self.emit_active_changed();
        }
    }

    fn emit_active_changed(&mut self) {
        let active = self.active_screen();
        if let Some(screen) = active {
            self.pending_events.push(NavigationEvent::StateChanged {
                stack_depth: self.stack.len() + self.modals.len(),
                active_route: screen.route_name.clone(),
            });
        }
    }

    /// Resolve a deep link URL to a navigation action.
    fn resolve_deep_link(&mut self, url: &str) {
        // Simple path matching: "/profile/123" matches "/profile/:id"
        for (name, route) in &self.routes {
            if let Some(pattern) = &route.path {
                if let Some(params) = match_path(pattern, url) {
                    let presentation = route.presentation;
                    let route_name = name.clone();
                    match presentation {
                        Presentation::Modal => {
                            self.push_screen(&route_name, params, Presentation::Modal);
                        }
                        _ => {
                            self.push_screen(&route_name, params, Presentation::Push);
                        }
                    }
                    return;
                }
            }
        }
        tracing::warn!(url = url, "No route matched deep link");
    }

    /// Get the currently active (visible) screen.
    pub fn active_screen(&self) -> Option<&Screen> {
        self.modals.last()
            .or_else(|| self.stack.last())
    }

    /// Get the current stack for DevTools.
    pub fn stack_snapshot(&self) -> Vec<&Screen> {
        self.stack.iter().chain(self.modals.iter()).collect()
    }

    /// Check if back navigation is possible.
    pub fn can_go_back(&self) -> bool {
        !self.modals.is_empty() || self.stack.len() > 1
    }
}

/// Simple path pattern matching.
/// Pattern: "/profile/:id" matches URL: "/profile/123" → {"id": "123"}
fn match_path(pattern: &str, url: &str) -> Option<HashMap<String, String>> {
    let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let url_parts: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();

    // Strip query string from URL
    let url_parts: Vec<&str> = url_parts.iter()
        .map(|p| p.split('?').next().unwrap_or(p))
        .collect();

    if pattern_parts.len() != url_parts.len() {
        return None;
    }

    let mut params = HashMap::new();

    for (pattern_part, url_part) in pattern_parts.iter().zip(url_parts.iter()) {
        if pattern_part.starts_with(':') {
            let param_name = &pattern_part[1..];
            params.insert(param_name.to_string(), url_part.to_string());
        } else if pattern_part != url_part {
            return None;
        }
    }

    Some(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_navigator() -> Navigator {
        let mut nav = Navigator::new();
        nav.register_route(RouteDefinition {
            name: "Home".to_string(),
            path: Some("/".to_string()),
            presentation: Presentation::Push,
            options: RouteOptions::default(),
        });
        nav.register_route(RouteDefinition {
            name: "Profile".to_string(),
            path: Some("/profile/:id".to_string()),
            presentation: Presentation::Push,
            options: RouteOptions { gesture_enabled: true, ..Default::default() },
        });
        nav.register_route(RouteDefinition {
            name: "Settings".to_string(),
            path: Some("/settings".to_string()),
            presentation: Presentation::Modal,
            options: RouteOptions::default(),
        });

        // Push initial screen
        nav.dispatch(NavigationAction::Push {
            route: "Home".to_string(),
            params: HashMap::new(),
        });

        nav
    }

    #[test]
    fn test_push_and_pop() {
        let mut nav = setup_navigator();
        assert_eq!(nav.stack.len(), 1);

        nav.dispatch(NavigationAction::Push {
            route: "Profile".to_string(),
            params: [("id".to_string(), "42".to_string())].into(),
        });
        assert_eq!(nav.stack.len(), 2);
        assert_eq!(nav.active_screen().unwrap().route_name, "Profile");

        nav.dispatch(NavigationAction::Pop);
        assert_eq!(nav.stack.len(), 1);
        assert_eq!(nav.active_screen().unwrap().route_name, "Home");
    }

    #[test]
    fn test_modal() {
        let mut nav = setup_navigator();

        nav.dispatch(NavigationAction::PresentModal {
            route: "Settings".to_string(),
            params: HashMap::new(),
        });
        assert_eq!(nav.modals.len(), 1);
        assert_eq!(nav.active_screen().unwrap().route_name, "Settings");

        // GoBack should dismiss modal first
        nav.dispatch(NavigationAction::GoBack);
        assert_eq!(nav.modals.len(), 0);
        assert_eq!(nav.active_screen().unwrap().route_name, "Home");
    }

    #[test]
    fn test_deep_link() {
        let mut nav = setup_navigator();

        nav.dispatch(NavigationAction::DeepLink {
            url: "/profile/99".to_string(),
        });
        assert_eq!(nav.stack.len(), 2);
        assert_eq!(nav.active_screen().unwrap().params.get("id").unwrap(), "99");
    }

    #[test]
    fn test_cannot_pop_root() {
        let mut nav = setup_navigator();
        nav.dispatch(NavigationAction::Pop);
        assert_eq!(nav.stack.len(), 1); // Root is preserved
    }

    #[test]
    fn test_path_matching() {
        let params = match_path("/profile/:id", "/profile/123").unwrap();
        assert_eq!(params.get("id").unwrap(), "123");

        assert!(match_path("/profile/:id", "/settings").is_none());
        assert!(match_path("/a/:b/:c", "/a/1/2").is_some());
    }
}
