//! Integration tests — full IR round-trip through the Engine.
//!
//! Exercises: JSON encode → IrBatch → Engine::apply_commit → platform ops.

use appscale_core::ir::{IrBatch, IrCommand};
use appscale_core::platform::{
    NativeHandle, PlatformBridge, PlatformCapability, PlatformError, PlatformId, PropValue,
    PropsDiff, ScreenSize, TextMetrics, TextStyle, ViewType,
};
use appscale_core::tree::NodeId;
use appscale_core::Engine;
use std::sync::{Arc, Mutex};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test mock platform (integration tests can't access #[cfg(test)] mocks)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Op {
    Create(ViewType, NodeId),
    Update(NativeHandle),
    Remove(NativeHandle),
    InsertChild(NativeHandle, NativeHandle, usize),
    RemoveChild(NativeHandle, NativeHandle),
}

struct TestPlatform {
    next_handle: Mutex<u64>,
    ops: Mutex<Vec<Op>>,
}

impl TestPlatform {
    fn new() -> Self {
        Self {
            next_handle: Mutex::new(1),
            ops: Mutex::new(Vec::new()),
        }
    }

    fn ops(&self) -> Vec<Op> {
        self.ops.lock().unwrap().clone()
    }

    fn clear(&self) {
        self.ops.lock().unwrap().clear();
    }
}

impl PlatformBridge for TestPlatform {
    fn platform_id(&self) -> PlatformId {
        PlatformId::Web
    }
    fn create_view(&self, view_type: ViewType, node_id: NodeId) -> NativeHandle {
        let mut h = self.next_handle.lock().unwrap();
        let handle = NativeHandle(*h);
        *h += 1;
        self.ops
            .lock()
            .unwrap()
            .push(Op::Create(view_type, node_id));
        handle
    }
    fn update_view(&self, handle: NativeHandle, _props: &PropsDiff) -> Result<(), PlatformError> {
        self.ops.lock().unwrap().push(Op::Update(handle));
        Ok(())
    }
    fn remove_view(&self, handle: NativeHandle) {
        self.ops.lock().unwrap().push(Op::Remove(handle));
    }
    fn insert_child(&self, parent: NativeHandle, child: NativeHandle, index: usize) {
        self.ops
            .lock()
            .unwrap()
            .push(Op::InsertChild(parent, child, index));
    }
    fn remove_child(&self, parent: NativeHandle, child: NativeHandle) {
        self.ops
            .lock()
            .unwrap()
            .push(Op::RemoveChild(parent, child));
    }
    fn measure_text(&self, text: &str, style: &TextStyle, _max_width: f32) -> TextMetrics {
        let fs = style.font_size.unwrap_or(14.0);
        TextMetrics {
            width: fs * 0.6 * text.len() as f32,
            height: fs * 1.2,
            baseline: fs,
            line_count: 1,
        }
    }
    fn screen_size(&self) -> ScreenSize {
        ScreenSize {
            width: 390.0,
            height: 844.0,
        }
    }
    fn scale_factor(&self) -> f32 {
        3.0
    }
    fn supports(&self, _cap: PlatformCapability) -> bool {
        false
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Integration tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Shorthand to reduce verbosity.
fn nid(v: u64) -> NodeId {
    NodeId(v)
}

fn make_platform() -> Arc<TestPlatform> {
    Arc::new(TestPlatform::new())
}

/// Full round-trip: JSON string → IrBatch → Engine → platform ops.
#[test]
fn json_round_trip() {
    let json = r#"{
        "commit_id": 1,
        "timestamp_ms": 0.0,
        "commands": [
            { "type": "create", "id": 100, "view_type": "Container", "props": {}, "style": {} },
            { "type": "set_root", "id": 100 },
            { "type": "create", "id": 101, "view_type": "Text", "props": {"text": "Hello"}, "style": {} },
            { "type": "append_child", "parent": 100, "child": 101 }
        ]
    }"#;

    let batch: IrBatch = serde_json::from_str(json).expect("should parse JSON");
    assert_eq!(batch.commit_id, 1);
    assert_eq!(batch.commands.len(), 4);

    let platform = make_platform();
    let mut engine = Engine::new(platform.clone());
    engine.apply_commit(&batch).expect("apply_commit");

    let ops = platform.ops();
    let creates: Vec<_> = ops.iter().filter(|o| matches!(o, Op::Create(..))).collect();
    assert_eq!(creates.len(), 2);
}

/// JSON encode → decode round-trip preserves data.
#[test]
fn json_encode_decode_round_trip() {
    use std::collections::HashMap;

    let batch = IrBatch {
        commit_id: 42,
        timestamp_ms: 123.456,
        commands: vec![
            IrCommand::CreateNode {
                id: nid(1),
                view_type: ViewType::Container,
                props: HashMap::new(),
                style: Default::default(),
            },
            IrCommand::SetRootNode { id: nid(1) },
            IrCommand::CreateNode {
                id: nid(2),
                view_type: ViewType::Text,
                props: {
                    let mut p = HashMap::new();
                    p.insert("text".to_string(), PropValue::String("Hello".to_string()));
                    p
                },
                style: Default::default(),
            },
            IrCommand::AppendChild {
                parent: nid(1),
                child: nid(2),
            },
        ],
    };

    let json = serde_json::to_string(&batch).expect("serialize");
    let decoded: IrBatch = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(decoded.commit_id, batch.commit_id);
    assert_eq!(decoded.timestamp_ms, batch.timestamp_ms);
    assert_eq!(decoded.commands.len(), batch.commands.len());
}

/// Multiple commits build up the tree incrementally.
#[test]
fn incremental_commits() {
    let platform = make_platform();
    let mut engine = Engine::new(platform.clone());

    let batch1 = IrBatch {
        commit_id: 1,
        timestamp_ms: 0.0,
        commands: vec![
            IrCommand::CreateNode {
                id: nid(1),
                view_type: ViewType::Container,
                props: std::collections::HashMap::new(),
                style: Default::default(),
            },
            IrCommand::SetRootNode { id: nid(1) },
        ],
    };
    engine.apply_commit(&batch1).unwrap();
    let ops1_count = platform.ops().len();
    assert!(ops1_count > 0);

    platform.clear();

    let batch2 = IrBatch {
        commit_id: 2,
        timestamp_ms: 16.0,
        commands: vec![
            IrCommand::CreateNode {
                id: nid(2),
                view_type: ViewType::Text,
                props: std::collections::HashMap::new(),
                style: Default::default(),
            },
            IrCommand::CreateNode {
                id: nid(3),
                view_type: ViewType::Button,
                props: std::collections::HashMap::new(),
                style: Default::default(),
            },
            IrCommand::AppendChild {
                parent: nid(1),
                child: nid(2),
            },
            IrCommand::AppendChild {
                parent: nid(1),
                child: nid(3),
            },
        ],
    };
    engine.apply_commit(&batch2).unwrap();
    let ops2 = platform.ops();
    let creates2: Vec<_> = ops2
        .iter()
        .filter(|o| matches!(o, Op::Create(..)))
        .collect();
    assert_eq!(creates2.len(), 2);
}

/// Props update triggers platform update but not create.
#[test]
fn props_update() {
    let platform = make_platform();
    let mut engine = Engine::new(platform.clone());

    let batch1 = IrBatch {
        commit_id: 1,
        timestamp_ms: 0.0,
        commands: vec![
            IrCommand::CreateNode {
                id: nid(1),
                view_type: ViewType::Text,
                props: {
                    let mut p = std::collections::HashMap::new();
                    p.insert("text".into(), PropValue::String("Old".into()));
                    p
                },
                style: Default::default(),
            },
            IrCommand::SetRootNode { id: nid(1) },
        ],
    };
    engine.apply_commit(&batch1).unwrap();
    platform.clear();

    let batch2 = IrBatch {
        commit_id: 2,
        timestamp_ms: 16.0,
        commands: vec![IrCommand::UpdateProps {
            id: nid(1),
            diff: {
                let mut d = PropsDiff::new();
                d.set("text", PropValue::String("New".into()));
                d
            },
        }],
    };
    engine.apply_commit(&batch2).unwrap();
    let ops2 = platform.ops();
    assert!(ops2.iter().all(|o| !matches!(o, Op::Create(..))));
    assert!(ops2.iter().any(|o| matches!(o, Op::Update(..))));
}

/// Remove child generates removal ops.
#[test]
fn remove_child() {
    let platform = make_platform();
    let mut engine = Engine::new(platform.clone());

    let batch1 = IrBatch {
        commit_id: 1,
        timestamp_ms: 0.0,
        commands: vec![
            IrCommand::CreateNode {
                id: nid(1),
                view_type: ViewType::Container,
                props: std::collections::HashMap::new(),
                style: Default::default(),
            },
            IrCommand::SetRootNode { id: nid(1) },
            IrCommand::CreateNode {
                id: nid(2),
                view_type: ViewType::Text,
                props: std::collections::HashMap::new(),
                style: Default::default(),
            },
            IrCommand::AppendChild {
                parent: nid(1),
                child: nid(2),
            },
        ],
    };
    engine.apply_commit(&batch1).unwrap();
    platform.clear();

    let batch2 = IrBatch {
        commit_id: 2,
        timestamp_ms: 16.0,
        commands: vec![IrCommand::RemoveChild {
            parent: nid(1),
            child: nid(2),
        }],
    };
    engine.apply_commit(&batch2).unwrap();
}

/// Sync call path: measure returns NotFound for unknown node.
#[test]
fn sync_measure_not_found() {
    use appscale_core::bridge::{SyncCall, SyncResult};

    let platform = make_platform();
    let engine = Engine::new(platform);

    let result = engine.handle_sync(&SyncCall::Measure { node_id: nid(9999) });
    assert!(matches!(result, SyncResult::NotFound));
}

/// Sync call path: isFocused returns false by default.
#[test]
fn sync_is_focused_default() {
    use appscale_core::bridge::{SyncCall, SyncResult};

    let platform = make_platform();
    let engine = Engine::new(platform);

    let result = engine.handle_sync(&SyncCall::IsFocused { node_id: nid(1) });
    match result {
        SyncResult::Bool { value } => assert!(!value),
        _ => panic!("Expected Bool result"),
    }
}

/// Malformed JSON fails gracefully.
#[test]
fn malformed_json_fails() {
    let bad_json = r#"{ "commit_id": 1, "commands": "not_an_array" }"#;
    let result: Result<IrBatch, _> = serde_json::from_str(bad_json);
    assert!(result.is_err());
}

/// Missing required fields fail.
#[test]
fn missing_fields_fail() {
    let incomplete = r#"{ "commit_id": 1 }"#;
    let result: Result<IrBatch, _> = serde_json::from_str(incomplete);
    assert!(result.is_err());
}

/// Invalid command type fails.
#[test]
fn invalid_command_type() {
    let bad_cmd = r#"{
        "commit_id": 1,
        "timestamp_ms": 0.0,
        "commands": [{ "type": "explode", "id": 1 }]
    }"#;
    let result: Result<IrBatch, _> = serde_json::from_str(bad_cmd);
    assert!(result.is_err());
}

/// Large batch processes without panic.
#[test]
fn large_batch() {
    let platform = make_platform();
    let mut engine = Engine::new(platform);

    let mut commands = vec![
        IrCommand::CreateNode {
            id: nid(0),
            view_type: ViewType::Container,
            props: std::collections::HashMap::new(),
            style: Default::default(),
        },
        IrCommand::SetRootNode { id: nid(0) },
    ];

    for i in 1..=500u64 {
        commands.push(IrCommand::CreateNode {
            id: nid(i),
            view_type: ViewType::Text,
            props: std::collections::HashMap::new(),
            style: Default::default(),
        });
        commands.push(IrCommand::AppendChild {
            parent: nid(0),
            child: nid(i),
        });
    }

    let batch = IrBatch {
        commit_id: 1,
        timestamp_ms: 0.0,
        commands,
    };

    engine
        .apply_commit(&batch)
        .expect("large batch should succeed");
}

/// Navigation dispatch round-trip.
#[test]
fn navigation_round_trip() {
    use appscale_core::navigation::NavigationAction;

    let platform = make_platform();
    let mut engine = Engine::new(platform);

    let events = engine.navigate(NavigationAction::Push {
        route: "Home".to_string(),
        params: std::collections::HashMap::new(),
    });
    assert!(!events.is_empty());

    let events2 = engine.navigate(NavigationAction::Push {
        route: "Details".to_string(),
        params: std::collections::HashMap::new(),
    });
    assert!(!events2.is_empty());

    let back_events = engine.navigate(NavigationAction::Pop);
    assert!(!back_events.is_empty());
}
