//! Unified Event System — pointer, keyboard, and gesture events.
//!
//! Event flow: Native → Rust → React
//! NOT: Native → React directly
//!
//! The dispatcher implements W3C-style capture → target → bubble propagation.
//! The gesture recognizer synthesizes tap/pan/swipe from raw pointer sequences.

use crate::layout::LayoutEngine;
use crate::tree::{NodeId, ShadowTree};
use rustc_hash::FxHashMap;
use std::time::{Duration, Instant};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Event types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Every input event in the framework.
#[derive(Debug, Clone)]
pub enum InputEvent {
    PointerDown(PointerEvent),
    PointerMove(PointerEvent),
    PointerUp(PointerEvent),
    PointerCancel(PointerEvent),
    Scroll(ScrollEvent),
    KeyDown(KeyboardEvent),
    KeyUp(KeyboardEvent),
    // Gestures (synthesized)
    Tap {
        x: f32,
        y: f32,
        target: Option<NodeId>,
    },
    DoubleTap {
        x: f32,
        y: f32,
        target: Option<NodeId>,
    },
    LongPress {
        x: f32,
        y: f32,
        target: Option<NodeId>,
    },
    Pan {
        dx: f32,
        dy: f32,
        vx: f32,
        vy: f32,
        target: Option<NodeId>,
        ended: bool,
    },
    Swipe {
        direction: SwipeDirection,
        velocity: f32,
        target: Option<NodeId>,
    },
}

#[derive(Debug, Clone)]
pub struct PointerEvent {
    pub pointer_id: u32,
    pub pointer_type: PointerType,
    pub screen_x: f32,
    pub screen_y: f32,
    pub pressure: f32,
    pub buttons: u32,
    pub modifiers: Modifiers,
    pub timestamp: Instant,
    pub target: Option<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerType {
    Mouse,
    Touch,
    Pen,
}

#[derive(Debug, Clone)]
pub struct ScrollEvent {
    pub delta_x: f32,
    pub delta_y: f32,
    pub modifiers: Modifiers,
    pub target: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    pub code: String,
    pub key: String,
    pub modifiers: Modifiers,
    pub is_repeat: bool,
    pub target: Option<NodeId>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Event handlers and dispatch
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub type HandlerFn = Box<dyn Fn(&InputEvent) -> HandlerResponse + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HandlerResponse {
    Continue,
    StopPropagation,
    PreventDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventPhase {
    Capture,
    Bubble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct HandlerKey {
    node_id: NodeId,
    event_kind: EventKind,
    phase: EventPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    PointerDown,
    PointerMove,
    PointerUp,
    PointerCancel,
    Scroll,
    KeyDown,
    KeyUp,
    Tap,
    DoubleTap,
    LongPress,
    Pan,
    Swipe,
}

impl InputEvent {
    pub fn kind(&self) -> EventKind {
        match self {
            InputEvent::PointerDown(_) => EventKind::PointerDown,
            InputEvent::PointerMove(_) => EventKind::PointerMove,
            InputEvent::PointerUp(_) => EventKind::PointerUp,
            InputEvent::PointerCancel(_) => EventKind::PointerCancel,
            InputEvent::Scroll(_) => EventKind::Scroll,
            InputEvent::KeyDown(_) => EventKind::KeyDown,
            InputEvent::KeyUp(_) => EventKind::KeyUp,
            InputEvent::Tap { .. } => EventKind::Tap,
            InputEvent::DoubleTap { .. } => EventKind::DoubleTap,
            InputEvent::LongPress { .. } => EventKind::LongPress,
            InputEvent::Pan { .. } => EventKind::Pan,
            InputEvent::Swipe { .. } => EventKind::Swipe,
        }
    }

    /// Get the screen position of this event (for hit testing).
    pub fn screen_position(&self) -> Option<(f32, f32)> {
        match self {
            InputEvent::PointerDown(e)
            | InputEvent::PointerMove(e)
            | InputEvent::PointerUp(e)
            | InputEvent::PointerCancel(e) => Some((e.screen_x, e.screen_y)),
            InputEvent::Tap { x, y, .. }
            | InputEvent::DoubleTap { x, y, .. }
            | InputEvent::LongPress { x, y, .. } => Some((*x, *y)),
            _ => None,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Dispatcher
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct EventResult {
    pub propagation_stopped: bool,
    pub default_prevented: bool,
}

pub struct EventDispatcher {
    handlers: FxHashMap<HandlerKey, Vec<HandlerFn>>,
    gesture: GestureRecognizer,
}

impl Default for EventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl EventDispatcher {
    pub fn new() -> Self {
        Self {
            handlers: FxHashMap::default(),
            gesture: GestureRecognizer::new(),
        }
    }

    /// Register a handler for a node + event kind + phase.
    pub fn add_handler(
        &mut self,
        node_id: NodeId,
        kind: EventKind,
        phase: EventPhase,
        handler: HandlerFn,
    ) {
        let key = HandlerKey {
            node_id,
            event_kind: kind,
            phase,
        };
        self.handlers.entry(key).or_default().push(handler);
    }

    /// Remove all handlers for a node (called when node is removed from tree).
    pub fn remove_handlers_for(&mut self, node_id: NodeId) {
        self.handlers.retain(|key, _| key.node_id != node_id);
    }

    /// Dispatch an event through the tree.
    pub fn dispatch(
        &mut self,
        mut event: InputEvent,
        layout: &LayoutEngine,
        tree: &ShadowTree,
    ) -> EventResult {
        let mut result = EventResult {
            propagation_stopped: false,
            default_prevented: false,
        };

        // Hit test to find target
        let target = if let Some((x, y)) = event.screen_position() {
            layout.hit_test(x, y).first().copied()
        } else {
            None
        };

        // Set target on the event
        self.set_target(&mut event, target);

        let target = match target {
            Some(t) => t,
            None => return result,
        };

        // Build propagation path: [root, ..., parent, target]
        let path = tree.ancestors(target);
        let kind = event.kind();

        // Capture phase (root → target)
        for &node_id in &path {
            if result.propagation_stopped {
                break;
            }
            let key = HandlerKey {
                node_id,
                event_kind: kind,
                phase: EventPhase::Capture,
            };
            if let Some(handlers) = self.handlers.get(&key) {
                for handler in handlers {
                    match handler(&event) {
                        HandlerResponse::StopPropagation => result.propagation_stopped = true,
                        HandlerResponse::PreventDefault => result.default_prevented = true,
                        HandlerResponse::Continue => {}
                    }
                    if result.propagation_stopped {
                        break;
                    }
                }
            }
        }

        // Bubble phase (target → root)
        for &node_id in path.iter().rev() {
            if result.propagation_stopped {
                break;
            }
            let key = HandlerKey {
                node_id,
                event_kind: kind,
                phase: EventPhase::Bubble,
            };
            if let Some(handlers) = self.handlers.get(&key) {
                for handler in handlers {
                    match handler(&event) {
                        HandlerResponse::StopPropagation => result.propagation_stopped = true,
                        HandlerResponse::PreventDefault => result.default_prevented = true,
                        HandlerResponse::Continue => {}
                    }
                    if result.propagation_stopped {
                        break;
                    }
                }
            }
        }

        // Feed to gesture recognizer (may produce tap/pan/swipe)
        if let Some(gestures) = self.gesture.process(&event) {
            for gesture in gestures {
                self.dispatch(gesture, layout, tree);
            }
        }

        result
    }

    fn set_target(&self, event: &mut InputEvent, target: Option<NodeId>) {
        match event {
            InputEvent::PointerDown(e)
            | InputEvent::PointerMove(e)
            | InputEvent::PointerUp(e)
            | InputEvent::PointerCancel(e) => {
                e.target = target;
            }
            InputEvent::Tap { target: t, .. }
            | InputEvent::DoubleTap { target: t, .. }
            | InputEvent::LongPress { target: t, .. }
            | InputEvent::Pan { target: t, .. }
            | InputEvent::Swipe { target: t, .. } => {
                *t = target;
            }
            _ => {}
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Gesture recognizer
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const TAP_MAX_DURATION: Duration = Duration::from_millis(300);
const TAP_MAX_DISTANCE: f32 = 10.0;
const LONG_PRESS_MIN: Duration = Duration::from_millis(500);
const PAN_THRESHOLD: f32 = 10.0;
const SWIPE_MIN_VELOCITY: f32 = 300.0;

struct PointerState {
    start: Instant,
    start_x: f32,
    start_y: f32,
    current_x: f32,
    current_y: f32,
    target: Option<NodeId>,
    panning: bool,
}

struct GestureRecognizer {
    pointers: FxHashMap<u32, PointerState>,
}

impl GestureRecognizer {
    fn new() -> Self {
        Self {
            pointers: FxHashMap::default(),
        }
    }

    fn process(&mut self, event: &InputEvent) -> Option<Vec<InputEvent>> {
        match event {
            InputEvent::PointerDown(e) => {
                self.pointers.insert(
                    e.pointer_id,
                    PointerState {
                        start: e.timestamp,
                        start_x: e.screen_x,
                        start_y: e.screen_y,
                        current_x: e.screen_x,
                        current_y: e.screen_y,
                        target: e.target,
                        panning: false,
                    },
                );
                None
            }
            InputEvent::PointerMove(e) => {
                let s = self.pointers.get_mut(&e.pointer_id)?;
                s.current_x = e.screen_x;
                s.current_y = e.screen_y;

                let dx = s.current_x - s.start_x;
                let dy = s.current_y - s.start_y;
                let dist = (dx * dx + dy * dy).sqrt();

                if !s.panning && dist > PAN_THRESHOLD {
                    s.panning = true;
                }

                if s.panning {
                    Some(vec![InputEvent::Pan {
                        dx,
                        dy,
                        vx: 0.0,
                        vy: 0.0,
                        target: s.target,
                        ended: false,
                    }])
                } else {
                    None
                }
            }
            InputEvent::PointerUp(e) => {
                let s = self.pointers.remove(&e.pointer_id)?;
                let dur = e.timestamp.duration_since(s.start);
                let dx = e.screen_x - s.start_x;
                let dy = e.screen_y - s.start_y;
                let dist = (dx * dx + dy * dy).sqrt();

                if s.panning {
                    let velocity = dist / dur.as_secs_f32();
                    let mut events = vec![InputEvent::Pan {
                        dx,
                        dy,
                        vx: dx / dur.as_secs_f32(),
                        vy: dy / dur.as_secs_f32(),
                        target: s.target,
                        ended: true,
                    }];

                    if velocity > SWIPE_MIN_VELOCITY {
                        let dir = if dx.abs() > dy.abs() {
                            if dx > 0.0 {
                                SwipeDirection::Right
                            } else {
                                SwipeDirection::Left
                            }
                        } else {
                            if dy > 0.0 {
                                SwipeDirection::Down
                            } else {
                                SwipeDirection::Up
                            }
                        };
                        events.push(InputEvent::Swipe {
                            direction: dir,
                            velocity,
                            target: s.target,
                        });
                    }
                    Some(events)
                } else if dur < TAP_MAX_DURATION && dist < TAP_MAX_DISTANCE {
                    Some(vec![InputEvent::Tap {
                        x: e.screen_x,
                        y: e.screen_y,
                        target: s.target,
                    }])
                } else if dur >= LONG_PRESS_MIN && dist < TAP_MAX_DISTANCE {
                    Some(vec![InputEvent::LongPress {
                        x: e.screen_x,
                        y: e.screen_y,
                        target: s.target,
                    }])
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Dimension, LayoutEngine, LayoutStyle};
    use crate::platform::mock::MockPlatform;
    use crate::platform::ViewType;
    use crate::tree::{NodeId, ShadowTree};
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    /// Builds a simple tree: root (400x400) → child (200x200 at 0,0)
    /// and returns (tree, layout, dispatcher)
    fn setup_simple(platform: &Arc<MockPlatform>) -> (ShadowTree, LayoutEngine, EventDispatcher) {
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();

        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(1),
                &LayoutStyle {
                    width: Dimension::Points(400.0),
                    height: Dimension::Points(400.0),
                    ..Default::default()
                },
            )
            .unwrap();

        tree.create_node(NodeId(2), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(2),
                &LayoutStyle {
                    width: Dimension::Points(200.0),
                    height: Dimension::Points(200.0),
                    ..Default::default()
                },
            )
            .unwrap();

        tree.set_root(NodeId(1));
        layout.set_root(NodeId(1));
        tree.append_child(NodeId(1), NodeId(2));
        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.compute(&tree, 400.0, 400.0, &**platform).unwrap();

        (tree, layout, EventDispatcher::new())
    }

    fn make_pointer_down(x: f32, y: f32) -> InputEvent {
        InputEvent::PointerDown(PointerEvent {
            pointer_id: 0,
            pointer_type: PointerType::Mouse,
            screen_x: x,
            screen_y: y,
            pressure: 1.0,
            buttons: 1,
            modifiers: Modifiers::default(),
            timestamp: Instant::now(),
            target: None,
        })
    }

    #[test]
    fn dispatch_bubble_phase() {
        let platform = Arc::new(MockPlatform::new());
        let (tree, layout, mut dispatcher) = setup_simple(&platform);

        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        dispatcher.add_handler(
            NodeId(2),
            EventKind::PointerDown,
            EventPhase::Bubble,
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                HandlerResponse::Continue
            }),
        );

        // Click inside child
        let event = make_pointer_down(50.0, 50.0);
        let result = dispatcher.dispatch(event, &layout, &tree);

        assert!(!result.propagation_stopped);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dispatch_capture_phase() {
        let platform = Arc::new(MockPlatform::new());
        let (tree, layout, mut dispatcher) = setup_simple(&platform);

        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let o1 = order.clone();
        let o2 = order.clone();

        // Root capture handler should fire first
        dispatcher.add_handler(
            NodeId(1),
            EventKind::PointerDown,
            EventPhase::Capture,
            Box::new(move |_| {
                o1.lock().unwrap().push(1);
                HandlerResponse::Continue
            }),
        );
        dispatcher.add_handler(
            NodeId(2),
            EventKind::PointerDown,
            EventPhase::Capture,
            Box::new(move |_| {
                o2.lock().unwrap().push(2);
                HandlerResponse::Continue
            }),
        );

        let event = make_pointer_down(50.0, 50.0);
        dispatcher.dispatch(event, &layout, &tree);

        let fired = order.lock().unwrap().clone();
        assert_eq!(
            fired,
            vec![1, 2],
            "Capture should fire root first, then target"
        );
    }

    #[test]
    fn dispatch_stop_propagation() {
        let platform = Arc::new(MockPlatform::new());
        let (tree, layout, mut dispatcher) = setup_simple(&platform);

        let parent_hit = Arc::new(AtomicU32::new(0));
        let p = parent_hit.clone();

        // Child stops propagation in capture phase
        dispatcher.add_handler(
            NodeId(2),
            EventKind::PointerDown,
            EventPhase::Capture,
            Box::new(|_| HandlerResponse::StopPropagation),
        );

        // Parent bubble handler should NOT fire
        dispatcher.add_handler(
            NodeId(1),
            EventKind::PointerDown,
            EventPhase::Bubble,
            Box::new(move |_| {
                p.fetch_add(1, Ordering::SeqCst);
                HandlerResponse::Continue
            }),
        );

        let event = make_pointer_down(50.0, 50.0);
        let result = dispatcher.dispatch(event, &layout, &tree);

        assert!(result.propagation_stopped);
        assert_eq!(
            parent_hit.load(Ordering::SeqCst),
            0,
            "Bubble should not fire after stop"
        );
    }

    #[test]
    fn dispatch_prevent_default() {
        let platform = Arc::new(MockPlatform::new());
        let (tree, layout, mut dispatcher) = setup_simple(&platform);

        dispatcher.add_handler(
            NodeId(2),
            EventKind::PointerDown,
            EventPhase::Bubble,
            Box::new(|_| HandlerResponse::PreventDefault),
        );

        let event = make_pointer_down(50.0, 50.0);
        let result = dispatcher.dispatch(event, &layout, &tree);

        assert!(result.default_prevented);
        assert!(!result.propagation_stopped);
    }

    #[test]
    fn dispatch_misses_when_outside_bounds() {
        let platform = Arc::new(MockPlatform::new());
        let (tree, layout, mut dispatcher) = setup_simple(&platform);

        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        dispatcher.add_handler(
            NodeId(2),
            EventKind::PointerDown,
            EventPhase::Bubble,
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                HandlerResponse::Continue
            }),
        );

        // Click outside all nodes (500, 500 is beyond 400x400 root)
        let event = make_pointer_down(500.0, 500.0);
        let result = dispatcher.dispatch(event, &layout, &tree);

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "Handler should not fire for miss"
        );
        assert!(!result.propagation_stopped);
    }

    #[test]
    fn dispatch_full_capture_target_bubble() {
        // Root(400x400) → Mid(300x300) → Leaf(100x100)
        let platform = Arc::new(MockPlatform::new());
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();

        for (id, w, h) in [(1, 400.0, 400.0), (2, 300.0, 300.0), (3, 100.0, 100.0)] {
            tree.create_node(NodeId(id), ViewType::Container, HashMap::new());
            layout
                .create_node(
                    NodeId(id),
                    &LayoutStyle {
                        width: Dimension::Points(w),
                        height: Dimension::Points(h),
                        ..Default::default()
                    },
                )
                .unwrap();
        }

        tree.set_root(NodeId(1));
        layout.set_root(NodeId(1));
        tree.append_child(NodeId(1), NodeId(2));
        tree.append_child(NodeId(2), NodeId(3));
        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.set_children_from_tree(NodeId(2), &tree).unwrap();
        layout.compute(&tree, 400.0, 400.0, &*platform).unwrap();

        let mut dispatcher = EventDispatcher::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        for id in [1u64, 2, 3] {
            let oc = order.clone();
            dispatcher.add_handler(
                NodeId(id),
                EventKind::PointerDown,
                EventPhase::Capture,
                Box::new(move |_| {
                    oc.lock().unwrap().push((id, "cap"));
                    HandlerResponse::Continue
                }),
            );

            let ob = order.clone();
            dispatcher.add_handler(
                NodeId(id),
                EventKind::PointerDown,
                EventPhase::Bubble,
                Box::new(move |_| {
                    ob.lock().unwrap().push((id, "bub"));
                    HandlerResponse::Continue
                }),
            );
        }

        let event = make_pointer_down(50.0, 50.0);
        dispatcher.dispatch(event, &layout, &tree);

        let fired = order.lock().unwrap().clone();
        // Capture: root → mid → leaf, then Bubble: leaf → mid → root
        assert_eq!(
            fired,
            vec![
                (1, "cap"),
                (2, "cap"),
                (3, "cap"),
                (3, "bub"),
                (2, "bub"),
                (1, "bub"),
            ]
        );
    }

    #[test]
    fn remove_handlers_cleanup() {
        let platform = Arc::new(MockPlatform::new());
        let (tree, layout, mut dispatcher) = setup_simple(&platform);

        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        dispatcher.add_handler(
            NodeId(2),
            EventKind::PointerDown,
            EventPhase::Bubble,
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                HandlerResponse::Continue
            }),
        );

        dispatcher.remove_handlers_for(NodeId(2));

        let event = make_pointer_down(50.0, 50.0);
        dispatcher.dispatch(event, &layout, &tree);

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "Handlers should be removed"
        );
    }

    #[test]
    fn gesture_tap_recognition() {
        let platform = Arc::new(MockPlatform::new());
        let (tree, layout, mut dispatcher) = setup_simple(&platform);

        let tapped = Arc::new(AtomicU32::new(0));
        let t = tapped.clone();
        dispatcher.add_handler(
            NodeId(2),
            EventKind::Tap,
            EventPhase::Bubble,
            Box::new(move |_| {
                t.fetch_add(1, Ordering::SeqCst);
                HandlerResponse::Continue
            }),
        );

        // Simulate quick pointer down + up at the same spot
        let now = Instant::now();
        let down = InputEvent::PointerDown(PointerEvent {
            pointer_id: 1,
            pointer_type: PointerType::Touch,
            screen_x: 50.0,
            screen_y: 50.0,
            pressure: 1.0,
            buttons: 1,
            modifiers: Modifiers::default(),
            timestamp: now,
            target: None,
        });
        let up = InputEvent::PointerUp(PointerEvent {
            pointer_id: 1,
            pointer_type: PointerType::Touch,
            screen_x: 50.0,
            screen_y: 50.0,
            pressure: 0.0,
            buttons: 0,
            modifiers: Modifiers::default(),
            timestamp: now + Duration::from_millis(50),
            target: None,
        });

        dispatcher.dispatch(down, &layout, &tree);
        dispatcher.dispatch(up, &layout, &tree);

        assert_eq!(tapped.load(Ordering::SeqCst), 1, "Tap gesture should fire");
    }

    #[test]
    fn event_kind_mapping() {
        assert_eq!(make_pointer_down(0.0, 0.0).kind(), EventKind::PointerDown);

        let scroll = InputEvent::Scroll(ScrollEvent {
            delta_x: 1.0,
            delta_y: 2.0,
            modifiers: Modifiers::default(),
            target: None,
        });
        assert_eq!(scroll.kind(), EventKind::Scroll);

        let key = InputEvent::KeyDown(KeyboardEvent {
            code: "KeyA".into(),
            key: "a".into(),
            modifiers: Modifiers::default(),
            is_repeat: false,
            target: None,
        });
        assert_eq!(key.kind(), EventKind::KeyDown);
    }

    #[test]
    fn screen_position_extraction() {
        let pe = make_pointer_down(42.0, 99.0);
        assert_eq!(pe.screen_position(), Some((42.0, 99.0)));

        let tap = InputEvent::Tap {
            x: 10.0,
            y: 20.0,
            target: None,
        };
        assert_eq!(tap.screen_position(), Some((10.0, 20.0)));

        let key = InputEvent::KeyDown(KeyboardEvent {
            code: "Space".into(),
            key: " ".into(),
            modifiers: Modifiers::default(),
            is_repeat: false,
            target: None,
        });
        assert_eq!(key.screen_position(), None);
    }
}
