//! Fuzz-style tests for IR deserialization.
//!
//! These exercise the JSON → IrBatch path with malformed, extreme, and
//! adversarial inputs. They ensure the deserializer never panics and
//! always returns Err for invalid data.

use appscale_core::ir::IrBatch;

/// Completely empty input.
#[test]
fn empty_input() {
    assert!(serde_json::from_str::<IrBatch>("").is_err());
}

/// Just whitespace.
#[test]
fn whitespace_only() {
    assert!(serde_json::from_str::<IrBatch>("   \n\t  ").is_err());
}

/// Null JSON value.
#[test]
fn null_value() {
    assert!(serde_json::from_str::<IrBatch>("null").is_err());
}

/// Boolean JSON value.
#[test]
fn boolean_value() {
    assert!(serde_json::from_str::<IrBatch>("true").is_err());
}

/// Number JSON value.
#[test]
fn number_value() {
    assert!(serde_json::from_str::<IrBatch>("42").is_err());
}

/// String JSON value.
#[test]
fn string_value() {
    assert!(serde_json::from_str::<IrBatch>(r#""hello""#).is_err());
}

/// Array at top level (expected object).
#[test]
fn array_top_level() {
    assert!(serde_json::from_str::<IrBatch>("[]").is_err());
}

/// Object missing all required fields.
#[test]
fn empty_object() {
    assert!(serde_json::from_str::<IrBatch>("{}").is_err());
}

/// Missing timestamp_ms.
#[test]
fn missing_timestamp() {
    let json = r#"{"commit_id": 1, "commands": []}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Missing commands array.
#[test]
fn missing_commands() {
    let json = r#"{"commit_id": 1, "timestamp_ms": 0.0}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Commands is wrong type (string instead of array).
#[test]
fn commands_wrong_type() {
    let json = r#"{"commit_id": 1, "timestamp_ms": 0.0, "commands": "invalid"}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Commands is a number.
#[test]
fn commands_number() {
    let json = r#"{"commit_id": 1, "timestamp_ms": 0.0, "commands": 42}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// commit_id is a string.
#[test]
fn commit_id_string() {
    let json = r#"{"commit_id": "abc", "timestamp_ms": 0.0, "commands": []}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Unknown command type.
#[test]
fn unknown_command_type() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[{"type":"destroy_all"}]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Command missing type field.
#[test]
fn command_missing_type() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[{"id":1}]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Create command missing required id.
#[test]
fn create_missing_id() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"create","view_type":"Text","props":{},"style":{}}
    ]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Create command with unknown view_type.
#[test]
fn create_unknown_view_type() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"create","id":1,"view_type":"Hologram","props":{},"style":{}}
    ]}"#;
    // Custom(String) is valid for unknown types, so this may parse.
    // Either way, it must not panic.
    let _ = serde_json::from_str::<IrBatch>(json);
}

/// Negative node IDs.
#[test]
fn negative_node_id() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"create","id":-1,"view_type":"Text","props":{},"style":{}}
    ]}"#;
    // NodeId is u64, so -1 should fail.
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Extremely large node ID.
#[test]
fn huge_node_id() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"create","id":18446744073709551615,"view_type":"Text","props":{},"style":{}}
    ]}"#;
    // u64::MAX — should parse successfully.
    let _ = serde_json::from_str::<IrBatch>(json);
}

/// Float node ID (should fail — NodeId is u64).
#[test]
fn float_node_id() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"create","id":1.5,"view_type":"Text","props":{},"style":{}}
    ]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Extremely large commit_id.
#[test]
fn huge_commit_id() {
    let json = r#"{"commit_id":18446744073709551615,"timestamp_ms":0.0,"commands":[]}"#;
    let batch = serde_json::from_str::<IrBatch>(json);
    assert!(batch.is_ok());
    assert_eq!(batch.unwrap().commit_id, u64::MAX);
}

/// Negative timestamp.
#[test]
fn negative_timestamp() {
    let json = r#"{"commit_id":1,"timestamp_ms":-100.0,"commands":[]}"#;
    // Negative timestamp is technically valid f64, should parse.
    let batch = serde_json::from_str::<IrBatch>(json);
    assert!(batch.is_ok());
}

/// NaN timestamp.
#[test]
fn nan_timestamp() {
    let json = r#"{"commit_id":1,"timestamp_ms":NaN,"commands":[]}"#;
    // NaN is not valid JSON.
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Infinity timestamp.
#[test]
fn infinity_timestamp() {
    let json = r#"{"commit_id":1,"timestamp_ms":Infinity,"commands":[]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Deeply nested JSON (object in commands array with deep nesting).
#[test]
fn deeply_nested() {
    // 128 levels of nesting
    let mut json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":["#.to_string();
    for _ in 0..128 {
        json.push_str(r#"{"nested":"#);
    }
    json.push_str("null");
    for _ in 0..128 {
        json.push('}');
    }
    json.push_str("]}");
    // Must not panic, should return error.
    let _ = serde_json::from_str::<IrBatch>(&json);
}

/// Empty commands array is valid.
#[test]
fn empty_commands_valid() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[]}"#;
    let batch = serde_json::from_str::<IrBatch>(json).unwrap();
    assert!(batch.commands.is_empty());
}

/// Very large commands array (1000 valid creates).
#[test]
fn large_commands_array() {
    let mut cmds = Vec::new();
    for i in 0..1000u64 {
        cmds.push(format!(
            r#"{{"type":"create","id":{},"view_type":"Text","props":{{}},"style":{{}}}}"#,
            i
        ));
    }
    let json = format!(
        r#"{{"commit_id":1,"timestamp_ms":0.0,"commands":[{}]}}"#,
        cmds.join(",")
    );
    let batch = serde_json::from_str::<IrBatch>(&json).unwrap();
    assert_eq!(batch.commands.len(), 1000);
}

/// Unicode in prop values.
#[test]
fn unicode_props() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"create","id":1,"view_type":"Text","props":{"text":"こんにちは🌍"},"style":{}}
    ]}"#;
    let batch = serde_json::from_str::<IrBatch>(json).unwrap();
    assert_eq!(batch.commands.len(), 1);
}

/// Null prop values.
#[test]
fn null_prop_value() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"create","id":1,"view_type":"Text","props":{"text":null},"style":{}}
    ]}"#;
    let batch = serde_json::from_str::<IrBatch>(json);
    // PropValue should handle null — either parse or reject cleanly.
    let _ = batch;
}

/// Duplicate fields in JSON object — serde rejects them when deny_unknown_fields is set.
#[test]
fn duplicate_fields() {
    let json = r#"{"commit_id":1,"commit_id":2,"timestamp_ms":0.0,"commands":[]}"#;
    // IrBatch rejects duplicate fields.
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Extra unknown fields should be ignored.
#[test]
fn extra_fields_ignored() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[],"extra":"data","more":123}"#;
    // serde default is to ignore unknown fields (can also deny — test both).
    let _ = serde_json::from_str::<IrBatch>(json);
}

/// Truncated JSON.
#[test]
fn truncated_json() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"com"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// Random bytes.
#[test]
fn random_bytes() {
    let garbage = b"\x00\xff\x80\xfe\x01\x02\x03";
    let s = String::from_utf8_lossy(garbage);
    assert!(serde_json::from_str::<IrBatch>(&s).is_err());
}

/// append_child with missing child field.
#[test]
fn append_child_missing_child() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"append_child","parent":1}
    ]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// remove_child with missing parent.
#[test]
fn remove_child_missing_parent() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"remove_child","child":1}
    ]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}

/// update_props with missing diff.
#[test]
fn update_props_missing_diff() {
    let json = r#"{"commit_id":1,"timestamp_ms":0.0,"commands":[
        {"type":"update_props","id":1}
    ]}"#;
    assert!(serde_json::from_str::<IrBatch>(json).is_err());
}
