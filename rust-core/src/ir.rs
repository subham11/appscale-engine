//! Binary IR Layer — the transport contract between React's reconciler and Rust.
//!
//! This is the framework's biggest innovation. Instead of:
//! - JSON bridge (React Native old arch: ~16-32ms per call)
//! - JSI object passing (React Native new arch: <1ms but still JS objects)
//!
//! We use a binary IR (FlatBuffers) that is:
//! - Deterministic: same input → same output
//! - Zero-copy deserializable
//! - Cross-language safe (JS → Rust with no marshaling)
//! - Replayable (critical for debugging, testing, and AI)
//!
//! Two transport modes:
//! - JSON (Phase 1 — DevTools, testing, debugging)
//! - FlatBuffers (Phase 2 — production performance)

use crate::tree::NodeId;
use crate::platform::{ViewType, PropsDiff, PropValue};
use crate::layout::LayoutStyle;
use crate::generated::flatbuf;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// A single command from the reconciler to the engine.
/// Each React commit produces a batch of these commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IrCommand {
    /// Create a new node with initial props and style.
    #[serde(rename = "create")]
    CreateNode {
        id: NodeId,
        view_type: ViewType,
        #[serde(default)]
        props: HashMap<String, PropValue>,
        #[serde(default)]
        style: LayoutStyle,
    },

    /// Update props on an existing node (only changed props).
    #[serde(rename = "update_props")]
    UpdateProps {
        id: NodeId,
        diff: PropsDiff,
    },

    /// Update layout style on an existing node.
    #[serde(rename = "update_style")]
    UpdateStyle {
        id: NodeId,
        style: LayoutStyle,
    },

    /// Append a child to a parent (at the end).
    #[serde(rename = "append_child")]
    AppendChild {
        parent: NodeId,
        child: NodeId,
    },

    /// Insert a child before another child.
    #[serde(rename = "insert_before")]
    InsertBefore {
        parent: NodeId,
        child: NodeId,
        before: NodeId,
    },

    /// Remove a child from its parent (and destroy the subtree).
    #[serde(rename = "remove_child")]
    RemoveChild {
        parent: NodeId,
        child: NodeId,
    },

    /// Set the root node of the tree.
    #[serde(rename = "set_root")]
    SetRootNode {
        id: NodeId,
    },
}

/// A batch of IR commands from a single React commit.
/// The engine processes these atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrBatch {
    /// Monotonically increasing commit ID.
    pub commit_id: u64,

    /// Timestamp (ms since app start).
    pub timestamp_ms: f64,

    /// The commands to execute, in order.
    pub commands: Vec<IrCommand>,
}

impl IrBatch {
    pub fn new(commit_id: u64) -> Self {
        Self {
            commit_id,
            timestamp_ms: 0.0,
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: IrCommand) {
        self.commands.push(cmd);
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }
}

// === JSON Transport (Phase 1 — bootstrap) ===

/// Decode an IR batch from JSON bytes.
/// Phase 1 uses JSON; Phase 2 replaces with FlatBuffers.
pub fn decode_batch(bytes: &[u8]) -> Result<IrBatch, IrError> {
    serde_json::from_slice(bytes).map_err(|e| IrError::DecodeFailed(e.to_string()))
}

/// Encode an IR batch to JSON bytes.
/// Used for DevTools replay and testing.
pub fn encode_batch(batch: &IrBatch) -> Result<Vec<u8>, IrError> {
    serde_json::to_vec(batch).map_err(|e| IrError::EncodeFailed(e.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum IrError {
    #[error("IR decode failed: {0}")]
    DecodeFailed(String),

    #[error("IR encode failed: {0}")]
    EncodeFailed(String),

    #[error("Unknown command type in FlatBuffers IR")]
    UnknownCommand,
}

// === FlatBuffers Transport (Phase 2 — production) ===

/// Decode an IR batch from FlatBuffers binary bytes.
/// Zero-copy deserialization — the buffer is read in-place.
pub fn decode_batch_flatbuf(bytes: &[u8]) -> Result<IrBatch, IrError> {
    let fb_batch = flatbuf::root_as_ir_batch(bytes)
        .map_err(|e| IrError::DecodeFailed(e.to_string()))?;

    let mut batch = IrBatch {
        commit_id: fb_batch.commit_id(),
        timestamp_ms: fb_batch.timestamp_ms(),
        commands: Vec::new(),
    };

    if let Some(commands) = fb_batch.commands() {
        batch.commands.reserve(commands.len());
        for fb_cmd in commands {
            let cmd = decode_fb_command(&fb_cmd)?;
            batch.commands.push(cmd);
        }
    }

    Ok(batch)
}

fn decode_fb_command(fb_cmd: &flatbuf::IrCommand<'_>) -> Result<IrCommand, IrError> {
    match fb_cmd.cmd_type() {
        flatbuf::Command::CreateNode => {
            let cn = fb_cmd.cmd_as_create_node().ok_or(IrError::UnknownCommand)?;
            let view_type = fb_view_type_to_engine(cn.view_type(), cn.custom_type());
            let props = cn.props().map(|p| fb_props_to_engine(&p)).unwrap_or_default();
            let style = cn.style().map(|s| fb_layout_to_engine(&s)).unwrap_or_default();
            Ok(IrCommand::CreateNode {
                id: NodeId(cn.id()),
                view_type,
                props,
                style,
            })
        }
        flatbuf::Command::UpdateProps => {
            let up = fb_cmd.cmd_as_update_props().ok_or(IrError::UnknownCommand)?;
            let diff = up.diff().map(|p| {
                let changes = fb_props_to_engine(&p);
                PropsDiff { changes }
            }).unwrap_or_default();
            Ok(IrCommand::UpdateProps {
                id: NodeId(up.id()),
                diff,
            })
        }
        flatbuf::Command::UpdateStyle => {
            let us = fb_cmd.cmd_as_update_style().ok_or(IrError::UnknownCommand)?;
            let style = us.style().map(|s| fb_layout_to_engine(&s)).unwrap_or_default();
            Ok(IrCommand::UpdateStyle {
                id: NodeId(us.id()),
                style,
            })
        }
        flatbuf::Command::AppendChild => {
            let ac = fb_cmd.cmd_as_append_child().ok_or(IrError::UnknownCommand)?;
            Ok(IrCommand::AppendChild {
                parent: NodeId(ac.parent()),
                child: NodeId(ac.child()),
            })
        }
        flatbuf::Command::InsertBefore => {
            let ib = fb_cmd.cmd_as_insert_before().ok_or(IrError::UnknownCommand)?;
            Ok(IrCommand::InsertBefore {
                parent: NodeId(ib.parent()),
                child: NodeId(ib.child()),
                before: NodeId(ib.before()),
            })
        }
        flatbuf::Command::RemoveChild => {
            let rc = fb_cmd.cmd_as_remove_child().ok_or(IrError::UnknownCommand)?;
            Ok(IrCommand::RemoveChild {
                parent: NodeId(rc.parent()),
                child: NodeId(rc.child()),
            })
        }
        flatbuf::Command::SetRoot => {
            let sr = fb_cmd.cmd_as_set_root().ok_or(IrError::UnknownCommand)?;
            Ok(IrCommand::SetRootNode { id: NodeId(sr.id()) })
        }
        _ => Err(IrError::UnknownCommand),
    }
}

/// Encode an IR batch to FlatBuffers binary bytes.
pub fn encode_batch_flatbuf(batch: &IrBatch) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::with_capacity(1024);

    let cmd_offsets: Vec<_> = batch.commands.iter().map(|cmd| {
        encode_fb_command(&mut fbb, cmd)
    }).collect();

    let commands = fbb.create_vector(&cmd_offsets);

    let fb_batch = flatbuf::IrBatch::create(&mut fbb, &flatbuf::IrBatchArgs {
        commit_id: batch.commit_id,
        timestamp_ms: batch.timestamp_ms,
        commands: Some(commands),
    });

    flatbuf::finish_ir_batch_buffer(&mut fbb, fb_batch);
    fbb.finished_data().to_vec()
}

fn encode_fb_command<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    cmd: &IrCommand,
) -> flatbuffers::WIPOffset<flatbuf::IrCommand<'a>> {
    match cmd {
        IrCommand::CreateNode { id, view_type, props, style } => {
            let (fb_vt, custom_str) = engine_view_type_to_fb(fbb, view_type);
            let fb_props = if props.is_empty() { None } else {
                Some(engine_props_to_fb(fbb, props))
            };
            let fb_style = Some(engine_layout_to_fb(fbb, style));
            let cn = flatbuf::CreateNode::create(fbb, &flatbuf::CreateNodeArgs {
                id: id.0,
                view_type: fb_vt,
                custom_type: custom_str,
                props: fb_props,
                style: fb_style,
            });
            flatbuf::IrCommand::create(fbb, &flatbuf::IrCommandArgs {
                cmd_type: flatbuf::Command::CreateNode,
                cmd: Some(cn.as_union_value()),
            })
        }
        IrCommand::UpdateProps { id, diff } => {
            let fb_diff = engine_props_to_fb_diff(fbb, diff);
            let up = flatbuf::UpdateProps::create(fbb, &flatbuf::UpdatePropsArgs {
                id: id.0,
                diff: Some(fb_diff),
            });
            flatbuf::IrCommand::create(fbb, &flatbuf::IrCommandArgs {
                cmd_type: flatbuf::Command::UpdateProps,
                cmd: Some(up.as_union_value()),
            })
        }
        IrCommand::UpdateStyle { id, style } => {
            let fb_style = engine_layout_to_fb(fbb, style);
            let us = flatbuf::UpdateStyle::create(fbb, &flatbuf::UpdateStyleArgs {
                id: id.0,
                style: Some(fb_style),
            });
            flatbuf::IrCommand::create(fbb, &flatbuf::IrCommandArgs {
                cmd_type: flatbuf::Command::UpdateStyle,
                cmd: Some(us.as_union_value()),
            })
        }
        IrCommand::AppendChild { parent, child } => {
            let ac = flatbuf::AppendChild::create(fbb, &flatbuf::AppendChildArgs {
                parent: parent.0,
                child: child.0,
            });
            flatbuf::IrCommand::create(fbb, &flatbuf::IrCommandArgs {
                cmd_type: flatbuf::Command::AppendChild,
                cmd: Some(ac.as_union_value()),
            })
        }
        IrCommand::InsertBefore { parent, child, before } => {
            let ib = flatbuf::InsertBefore::create(fbb, &flatbuf::InsertBeforeArgs {
                parent: parent.0,
                child: child.0,
                before: before.0,
            });
            flatbuf::IrCommand::create(fbb, &flatbuf::IrCommandArgs {
                cmd_type: flatbuf::Command::InsertBefore,
                cmd: Some(ib.as_union_value()),
            })
        }
        IrCommand::RemoveChild { parent, child } => {
            let rc = flatbuf::RemoveChild::create(fbb, &flatbuf::RemoveChildArgs {
                parent: parent.0,
                child: child.0,
            });
            flatbuf::IrCommand::create(fbb, &flatbuf::IrCommandArgs {
                cmd_type: flatbuf::Command::RemoveChild,
                cmd: Some(rc.as_union_value()),
            })
        }
        IrCommand::SetRootNode { id } => {
            let sr = flatbuf::SetRoot::create(fbb, &flatbuf::SetRootArgs { id: id.0 });
            flatbuf::IrCommand::create(fbb, &flatbuf::IrCommandArgs {
                cmd_type: flatbuf::Command::SetRoot,
                cmd: Some(sr.as_union_value()),
            })
        }
    }
}

// === Type conversion helpers ===

fn fb_view_type_to_engine(vt: flatbuf::ViewType, custom: Option<&str>) -> ViewType {
    match vt {
        flatbuf::ViewType::Container => ViewType::Container,
        flatbuf::ViewType::Text => ViewType::Text,
        flatbuf::ViewType::TextInput => ViewType::TextInput,
        flatbuf::ViewType::Image => ViewType::Image,
        flatbuf::ViewType::ScrollView => ViewType::ScrollView,
        flatbuf::ViewType::Button => ViewType::Button,
        flatbuf::ViewType::Switch => ViewType::Switch,
        flatbuf::ViewType::Slider => ViewType::Slider,
        flatbuf::ViewType::ActivityIndicator => ViewType::ActivityIndicator,
        flatbuf::ViewType::DatePicker => ViewType::DatePicker,
        flatbuf::ViewType::Modal => ViewType::Modal,
        flatbuf::ViewType::BottomSheet => ViewType::BottomSheet,
        flatbuf::ViewType::MenuBar => ViewType::MenuBar,
        flatbuf::ViewType::TitleBar => ViewType::TitleBar,
        flatbuf::ViewType::Custom => ViewType::Custom(custom.unwrap_or("").to_string()),
        _ => ViewType::Container,
    }
}

fn engine_view_type_to_fb<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    vt: &ViewType,
) -> (flatbuf::ViewType, Option<flatbuffers::WIPOffset<&'a str>>) {
    match vt {
        ViewType::Container => (flatbuf::ViewType::Container, None),
        ViewType::Text => (flatbuf::ViewType::Text, None),
        ViewType::TextInput => (flatbuf::ViewType::TextInput, None),
        ViewType::Image => (flatbuf::ViewType::Image, None),
        ViewType::ScrollView => (flatbuf::ViewType::ScrollView, None),
        ViewType::Button => (flatbuf::ViewType::Button, None),
        ViewType::Switch => (flatbuf::ViewType::Switch, None),
        ViewType::Slider => (flatbuf::ViewType::Slider, None),
        ViewType::ActivityIndicator => (flatbuf::ViewType::ActivityIndicator, None),
        ViewType::DatePicker => (flatbuf::ViewType::DatePicker, None),
        ViewType::Modal => (flatbuf::ViewType::Modal, None),
        ViewType::BottomSheet => (flatbuf::ViewType::BottomSheet, None),
        ViewType::MenuBar => (flatbuf::ViewType::MenuBar, None),
        ViewType::TitleBar => (flatbuf::ViewType::TitleBar, None),
        ViewType::Custom(name) => {
            let s = fbb.create_string(name);
            (flatbuf::ViewType::Custom, Some(s))
        }
    }
}

fn fb_props_to_engine(diff: &flatbuf::PropsDiff<'_>) -> HashMap<String, PropValue> {
    let mut map = HashMap::new();
    if let Some(changes) = diff.changes() {
        for entry in changes {
            let key = entry.key().to_string();
            let value = match entry.value_type() {
                flatbuf::PropValueUnion::StringVal => {
                    entry.value_as_string_val()
                        .map(|v| PropValue::String(v.value().unwrap_or("").to_string()))
                        .unwrap_or(PropValue::Null)
                }
                flatbuf::PropValueUnion::FloatVal => {
                    entry.value_as_float_val()
                        .map(|v| PropValue::F64(v.value() as f64))
                        .unwrap_or(PropValue::Null)
                }
                flatbuf::PropValueUnion::IntVal => {
                    entry.value_as_int_val()
                        .map(|v| PropValue::I32(v.value()))
                        .unwrap_or(PropValue::Null)
                }
                flatbuf::PropValueUnion::BoolVal => {
                    entry.value_as_bool_val()
                        .map(|v| PropValue::Bool(v.value()))
                        .unwrap_or(PropValue::Null)
                }
                flatbuf::PropValueUnion::ColorVal => {
                    entry.value_as_color_val()
                        .and_then(|v| v.value().map(|c| PropValue::Color(crate::platform::Color {
                            r: c.r(),
                            g: c.g(),
                            b: c.b(),
                            a: c.a(),
                        })))
                        .unwrap_or(PropValue::Null)
                }
                _ => PropValue::Null,
            };
            map.insert(key, value);
        }
    }
    map
}

fn engine_props_to_fb<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    props: &HashMap<String, PropValue>,
) -> flatbuffers::WIPOffset<flatbuf::PropsDiff<'a>> {
    let entries: Vec<_> = props.iter().map(|(key, value)| {
        encode_prop_entry(fbb, key, value)
    }).collect();
    let changes = fbb.create_vector(&entries);
    flatbuf::PropsDiff::create(fbb, &flatbuf::PropsDiffArgs {
        changes: Some(changes),
    })
}

fn engine_props_to_fb_diff<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    diff: &PropsDiff,
) -> flatbuffers::WIPOffset<flatbuf::PropsDiff<'a>> {
    engine_props_to_fb(fbb, &diff.changes)
}

fn encode_prop_entry<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    key: &str,
    value: &PropValue,
) -> flatbuffers::WIPOffset<flatbuf::PropEntry<'a>> {
    let key_offset = fbb.create_string(key);
    let (value_type, value_offset) = match value {
        PropValue::String(s) => {
            let sv = fbb.create_string(s);
            let val = flatbuf::StringVal::create(fbb, &flatbuf::StringValArgs { value: Some(sv) });
            (flatbuf::PropValueUnion::StringVal, val.as_union_value())
        }
        PropValue::F32(f) => {
            let val = flatbuf::FloatVal::create(fbb, &flatbuf::FloatValArgs { value: *f });
            (flatbuf::PropValueUnion::FloatVal, val.as_union_value())
        }
        PropValue::F64(f) => {
            let val = flatbuf::FloatVal::create(fbb, &flatbuf::FloatValArgs { value: *f as f32 });
            (flatbuf::PropValueUnion::FloatVal, val.as_union_value())
        }
        PropValue::I32(i) => {
            let val = flatbuf::IntVal::create(fbb, &flatbuf::IntValArgs { value: *i });
            (flatbuf::PropValueUnion::IntVal, val.as_union_value())
        }
        PropValue::Bool(b) => {
            let val = flatbuf::BoolVal::create(fbb, &flatbuf::BoolValArgs { value: *b });
            (flatbuf::PropValueUnion::BoolVal, val.as_union_value())
        }
        PropValue::Color(c) => {
            let fb_color = flatbuf::Color::new(c.r, c.g, c.b, c.a);
            let val = flatbuf::ColorVal::create(fbb, &flatbuf::ColorValArgs {
                value: Some(&fb_color),
            });
            (flatbuf::PropValueUnion::ColorVal, val.as_union_value())
        }
        PropValue::Rect { .. } | PropValue::Null => {
            (flatbuf::PropValueUnion::NONE, flatbuffers::WIPOffset::<flatbuf::StringVal>::new(0).as_union_value())
        }
    };
    flatbuf::PropEntry::create(fbb, &flatbuf::PropEntryArgs {
        key: Some(key_offset),
        value_type,
        value: Some(value_offset),
    })
}

fn fb_layout_to_engine(ls: &flatbuf::LayoutStyle<'_>) -> LayoutStyle {
    use crate::layout;

    let display = match ls.display() {
        flatbuf::Display::Flex => layout::Display::Flex,
        flatbuf::Display::Grid => layout::Display::Grid,
        flatbuf::Display::None => layout::Display::None,
        _ => layout::Display::Flex,
    };
    let position = match ls.position() {
        flatbuf::Position::Relative => layout::Position::Relative,
        flatbuf::Position::Absolute => layout::Position::Absolute,
        _ => layout::Position::Relative,
    };
    let flex_direction = match ls.flex_direction() {
        flatbuf::FlexDirection::Column => layout::FlexDirection::Column,
        flatbuf::FlexDirection::Row => layout::FlexDirection::Row,
        flatbuf::FlexDirection::ColumnReverse => layout::FlexDirection::ColumnReverse,
        flatbuf::FlexDirection::RowReverse => layout::FlexDirection::RowReverse,
        _ => layout::FlexDirection::Column,
    };
    let flex_wrap = match ls.flex_wrap() {
        flatbuf::FlexWrap::NoWrap => layout::FlexWrap::NoWrap,
        flatbuf::FlexWrap::Wrap => layout::FlexWrap::Wrap,
        flatbuf::FlexWrap::WrapReverse => layout::FlexWrap::WrapReverse,
        _ => layout::FlexWrap::NoWrap,
    };
    let justify_content = match ls.justify_content() {
        flatbuf::JustifyContent::FlexStart => Some(layout::JustifyContent::FlexStart),
        flatbuf::JustifyContent::FlexEnd => Some(layout::JustifyContent::FlexEnd),
        flatbuf::JustifyContent::Center => Some(layout::JustifyContent::Center),
        flatbuf::JustifyContent::SpaceBetween => Some(layout::JustifyContent::SpaceBetween),
        flatbuf::JustifyContent::SpaceAround => Some(layout::JustifyContent::SpaceAround),
        flatbuf::JustifyContent::SpaceEvenly => Some(layout::JustifyContent::SpaceEvenly),
        _ => None,
    };
    let align_items = match ls.align_items() {
        flatbuf::AlignItems::FlexStart => Some(layout::AlignItems::FlexStart),
        flatbuf::AlignItems::FlexEnd => Some(layout::AlignItems::FlexEnd),
        flatbuf::AlignItems::Center => Some(layout::AlignItems::Center),
        flatbuf::AlignItems::Stretch => Some(layout::AlignItems::Stretch),
        flatbuf::AlignItems::Baseline => Some(layout::AlignItems::Baseline),
        _ => None,
    };
    let overflow = match ls.overflow() {
        flatbuf::Overflow::Visible => layout::Overflow::Visible,
        flatbuf::Overflow::Hidden => layout::Overflow::Hidden,
        flatbuf::Overflow::Scroll => layout::Overflow::Scroll,
        _ => layout::Overflow::Visible,
    };

    LayoutStyle {
        display,
        position,
        flex_direction,
        flex_wrap,
        flex_grow: ls.flex_grow(),
        flex_shrink: ls.flex_shrink(),
        justify_content,
        align_items,
        width: fb_dimension_to_engine(ls.width()),
        height: fb_dimension_to_engine(ls.height()),
        min_width: fb_dimension_to_engine(ls.min_width()),
        min_height: fb_dimension_to_engine(ls.min_height()),
        max_width: fb_dimension_to_engine(ls.max_width()),
        max_height: fb_dimension_to_engine(ls.max_height()),
        aspect_ratio: if ls.aspect_ratio() == 0.0 { None } else { Some(ls.aspect_ratio()) },
        margin: ls.margin().map(fb_edges_to_engine).unwrap_or_default(),
        padding: ls.padding().map(fb_edges_to_engine).unwrap_or_default(),
        gap: ls.gap(),
        overflow,
    }
}

fn fb_dimension_to_engine(d: Option<&flatbuf::Dimension>) -> crate::layout::Dimension {
    match d {
        None => crate::layout::Dimension::Auto,
        Some(dim) => match dim.type_() {
            flatbuf::DimensionType::Auto => crate::layout::Dimension::Auto,
            flatbuf::DimensionType::Points => crate::layout::Dimension::Points(dim.value()),
            flatbuf::DimensionType::Percent => crate::layout::Dimension::Percent(dim.value()),
            _ => crate::layout::Dimension::Auto,
        },
    }
}

fn fb_edges_to_engine(e: &flatbuf::Edges) -> crate::layout::Edges {
    crate::layout::Edges {
        top: e.top(),
        right: e.right(),
        bottom: e.bottom(),
        left: e.left(),
    }
}

fn engine_layout_to_fb<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    style: &LayoutStyle,
) -> flatbuffers::WIPOffset<flatbuf::LayoutStyle<'a>> {
    use crate::layout;

    let display = match style.display {
        layout::Display::Flex => flatbuf::Display::Flex,
        layout::Display::Grid => flatbuf::Display::Grid,
        layout::Display::None => flatbuf::Display::None,
    };
    let position = match style.position {
        layout::Position::Relative => flatbuf::Position::Relative,
        layout::Position::Absolute => flatbuf::Position::Absolute,
    };
    let flex_direction = match style.flex_direction {
        layout::FlexDirection::Column => flatbuf::FlexDirection::Column,
        layout::FlexDirection::Row => flatbuf::FlexDirection::Row,
        layout::FlexDirection::ColumnReverse => flatbuf::FlexDirection::ColumnReverse,
        layout::FlexDirection::RowReverse => flatbuf::FlexDirection::RowReverse,
    };
    let flex_wrap = match style.flex_wrap {
        layout::FlexWrap::NoWrap => flatbuf::FlexWrap::NoWrap,
        layout::FlexWrap::Wrap => flatbuf::FlexWrap::Wrap,
        layout::FlexWrap::WrapReverse => flatbuf::FlexWrap::WrapReverse,
    };
    let justify_content = match style.justify_content {
        Some(layout::JustifyContent::FlexStart) | None => flatbuf::JustifyContent::FlexStart,
        Some(layout::JustifyContent::FlexEnd) => flatbuf::JustifyContent::FlexEnd,
        Some(layout::JustifyContent::Center) => flatbuf::JustifyContent::Center,
        Some(layout::JustifyContent::SpaceBetween) => flatbuf::JustifyContent::SpaceBetween,
        Some(layout::JustifyContent::SpaceAround) => flatbuf::JustifyContent::SpaceAround,
        Some(layout::JustifyContent::SpaceEvenly) => flatbuf::JustifyContent::SpaceEvenly,
    };
    let align_items = match style.align_items {
        Some(layout::AlignItems::FlexStart) | None => flatbuf::AlignItems::FlexStart,
        Some(layout::AlignItems::FlexEnd) => flatbuf::AlignItems::FlexEnd,
        Some(layout::AlignItems::Center) => flatbuf::AlignItems::Center,
        Some(layout::AlignItems::Stretch) => flatbuf::AlignItems::Stretch,
        Some(layout::AlignItems::Baseline) => flatbuf::AlignItems::Baseline,
    };
    let overflow = match style.overflow {
        layout::Overflow::Visible => flatbuf::Overflow::Visible,
        layout::Overflow::Hidden => flatbuf::Overflow::Hidden,
        layout::Overflow::Scroll => flatbuf::Overflow::Scroll,
    };

    let width = engine_dimension_to_fb(&style.width);
    let height = engine_dimension_to_fb(&style.height);
    let min_width = engine_dimension_to_fb(&style.min_width);
    let min_height = engine_dimension_to_fb(&style.min_height);
    let max_width = engine_dimension_to_fb(&style.max_width);
    let max_height = engine_dimension_to_fb(&style.max_height);
    let margin = flatbuf::Edges::new(
        style.margin.top, style.margin.right, style.margin.bottom, style.margin.left,
    );
    let padding = flatbuf::Edges::new(
        style.padding.top, style.padding.right, style.padding.bottom, style.padding.left,
    );

    flatbuf::LayoutStyle::create(fbb, &flatbuf::LayoutStyleArgs {
        display,
        position,
        flex_direction,
        flex_wrap,
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        justify_content,
        align_items,
        width: Some(&width),
        height: Some(&height),
        min_width: Some(&min_width),
        min_height: Some(&min_height),
        max_width: Some(&max_width),
        max_height: Some(&max_height),
        aspect_ratio: style.aspect_ratio.unwrap_or(0.0),
        margin: Some(&margin),
        padding: Some(&padding),
        gap: style.gap,
        overflow,
    })
}

fn engine_dimension_to_fb(dim: &crate::layout::Dimension) -> flatbuf::Dimension {
    match dim {
        crate::layout::Dimension::Auto => flatbuf::Dimension::new(flatbuf::DimensionType::Auto, 0.0),
        crate::layout::Dimension::Points(v) => flatbuf::Dimension::new(flatbuf::DimensionType::Points, *v),
        crate::layout::Dimension::Percent(v) => flatbuf::Dimension::new(flatbuf::DimensionType::Percent, *v),
    }
}

// === Serde impls for NodeId (serialize as u64) ===

impl Serialize for NodeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = u64::deserialize(deserializer)?;
        Ok(NodeId(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_json() {
        let mut batch = IrBatch::new(1);
        batch.push(IrCommand::CreateNode {
            id: NodeId(1),
            view_type: ViewType::Container,
            props: HashMap::new(),
            style: LayoutStyle::default(),
        });
        batch.push(IrCommand::CreateNode {
            id: NodeId(2),
            view_type: ViewType::Text,
            props: {
                let mut p = HashMap::new();
                p.insert("text".to_string(), PropValue::String("Hello".to_string()));
                p
            },
            style: LayoutStyle::default(),
        });
        batch.push(IrCommand::AppendChild {
            parent: NodeId(1),
            child: NodeId(2),
        });
        batch.push(IrCommand::SetRootNode { id: NodeId(1) });

        let encoded = encode_batch(&batch).unwrap();
        let decoded = decode_batch(&encoded).unwrap();

        assert_eq!(decoded.commit_id, 1);
        assert_eq!(decoded.commands.len(), 4);
    }

    #[test]
    fn test_roundtrip_flatbuf() {
        let mut batch = IrBatch::new(42);
        batch.timestamp_ms = 123.456;
        batch.push(IrCommand::CreateNode {
            id: NodeId(1),
            view_type: ViewType::Container,
            props: HashMap::new(),
            style: LayoutStyle::default(),
        });
        batch.push(IrCommand::CreateNode {
            id: NodeId(2),
            view_type: ViewType::Text,
            props: {
                let mut p = HashMap::new();
                p.insert("text".to_string(), PropValue::String("Hello FlatBuf".to_string()));
                p.insert("fontSize".to_string(), PropValue::F32(16.0));
                p.insert("bold".to_string(), PropValue::Bool(true));
                p.insert("count".to_string(), PropValue::I32(7));
                p
            },
            style: LayoutStyle {
                display: crate::layout::Display::Flex,
                flex_direction: crate::layout::FlexDirection::Row,
                width: crate::layout::Dimension::Points(100.0),
                height: crate::layout::Dimension::Percent(50.0),
                margin: crate::layout::Edges { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 },
                ..LayoutStyle::default()
            },
        });
        batch.push(IrCommand::AppendChild { parent: NodeId(1), child: NodeId(2) });
        batch.push(IrCommand::UpdateProps {
            id: NodeId(2),
            diff: {
                let mut d = PropsDiff::new();
                d.set("text", PropValue::String("Updated".to_string()));
                d
            },
        });
        batch.push(IrCommand::UpdateStyle {
            id: NodeId(2),
            style: LayoutStyle {
                flex_grow: 1.0,
                ..LayoutStyle::default()
            },
        });
        batch.push(IrCommand::InsertBefore {
            parent: NodeId(1),
            child: NodeId(3),
            before: NodeId(2),
        });
        batch.push(IrCommand::RemoveChild { parent: NodeId(1), child: NodeId(3) });
        batch.push(IrCommand::SetRootNode { id: NodeId(1) });

        // Encode to FlatBuffers
        let encoded = encode_batch_flatbuf(&batch);
        assert!(!encoded.is_empty());

        // Decode from FlatBuffers
        let decoded = decode_batch_flatbuf(&encoded).unwrap();
        assert_eq!(decoded.commit_id, 42);
        assert_eq!(decoded.timestamp_ms, 123.456);
        assert_eq!(decoded.commands.len(), 8);

        // Verify CreateNode
        match &decoded.commands[0] {
            IrCommand::CreateNode { id, view_type, .. } => {
                assert_eq!(*id, NodeId(1));
                assert_eq!(*view_type, ViewType::Container);
            }
            _ => panic!("Expected CreateNode"),
        }

        // Verify second CreateNode with props and style
        match &decoded.commands[1] {
            IrCommand::CreateNode { id, view_type, props, style } => {
                assert_eq!(*id, NodeId(2));
                assert_eq!(*view_type, ViewType::Text);
                assert_eq!(props.len(), 4);
                match &props["text"] {
                    PropValue::String(s) => assert_eq!(s, "Hello FlatBuf"),
                    _ => panic!("Expected String prop"),
                }
                assert!(matches!(style.flex_direction, crate::layout::FlexDirection::Row));
                assert!(matches!(style.width, crate::layout::Dimension::Points(v) if (v - 100.0).abs() < 0.01));
            }
            _ => panic!("Expected CreateNode"),
        }

        // Verify AppendChild
        match &decoded.commands[2] {
            IrCommand::AppendChild { parent, child } => {
                assert_eq!(*parent, NodeId(1));
                assert_eq!(*child, NodeId(2));
            }
            _ => panic!("Expected AppendChild"),
        }

        // Verify SetRootNode
        match &decoded.commands[7] {
            IrCommand::SetRootNode { id } => assert_eq!(*id, NodeId(1)),
            _ => panic!("Expected SetRootNode"),
        }
    }

    #[test]
    fn test_flatbuf_custom_view_type() {
        let mut batch = IrBatch::new(1);
        batch.push(IrCommand::CreateNode {
            id: NodeId(99),
            view_type: ViewType::Custom("MyWidget".to_string()),
            props: HashMap::new(),
            style: LayoutStyle::default(),
        });

        let encoded = encode_batch_flatbuf(&batch);
        let decoded = decode_batch_flatbuf(&encoded).unwrap();

        match &decoded.commands[0] {
            IrCommand::CreateNode { view_type, .. } => {
                assert_eq!(*view_type, ViewType::Custom("MyWidget".to_string()));
            }
            _ => panic!("Expected CreateNode"),
        }
    }

    #[test]
    fn test_flatbuf_color_prop() {
        let mut batch = IrBatch::new(1);
        batch.push(IrCommand::CreateNode {
            id: NodeId(1),
            view_type: ViewType::Container,
            props: {
                let mut p = HashMap::new();
                p.insert("bg".to_string(), PropValue::Color(crate::platform::Color::rgba(255, 0, 128, 0.5)));
                p
            },
            style: LayoutStyle::default(),
        });

        let encoded = encode_batch_flatbuf(&batch);
        let decoded = decode_batch_flatbuf(&encoded).unwrap();

        match &decoded.commands[0] {
            IrCommand::CreateNode { props, .. } => {
                match &props["bg"] {
                    PropValue::Color(c) => {
                        assert_eq!(c.r, 255);
                        assert_eq!(c.g, 0);
                        assert_eq!(c.b, 128);
                        assert!((c.a - 0.5).abs() < 0.01);
                    }
                    _ => panic!("Expected Color prop"),
                }
            }
            _ => panic!("Expected CreateNode"),
        }
    }

    #[test]
    fn test_flatbuf_size_vs_json() {
        let mut batch = IrBatch::new(1);
        for i in 0..100 {
            batch.push(IrCommand::CreateNode {
                id: NodeId(i),
                view_type: ViewType::Container,
                props: HashMap::new(),
                style: LayoutStyle::default(),
            });
        }

        let json_bytes = encode_batch(&batch).unwrap();
        let fb_bytes = encode_batch_flatbuf(&batch);

        // FlatBuffers should be smaller than JSON for structured data
        assert!(fb_bytes.len() < json_bytes.len(),
            "FlatBuffers ({} bytes) should be smaller than JSON ({} bytes)",
            fb_bytes.len(), json_bytes.len());
    }

    /// Build a realistic IrBatch with N nodes for benchmarking.
    fn make_bench_batch(n: u64) -> IrBatch {
        let mut batch = IrBatch::new(1);
        for i in 1..=n {
            batch.push(IrCommand::CreateNode {
                id: NodeId(i),
                view_type: if i % 5 == 0 { ViewType::Text } else { ViewType::Container },
                props: {
                    let mut p = HashMap::new();
                    p.insert("label".to_string(), PropValue::String(format!("node_{i}")));
                    if i % 3 == 0 {
                        p.insert("fontSize".to_string(), PropValue::F32(14.0));
                        p.insert("bold".to_string(), PropValue::Bool(true));
                    }
                    p
                },
                style: LayoutStyle {
                    width: crate::layout::Dimension::Points(100.0),
                    height: crate::layout::Dimension::Points(40.0),
                    margin: crate::layout::Edges { top: 4.0, right: 4.0, bottom: 4.0, left: 4.0 },
                    ..LayoutStyle::default()
                },
            });
            if i > 1 {
                batch.push(IrCommand::AppendChild {
                    parent: NodeId(((i - 2) / 4) + 1),
                    child: NodeId(i),
                });
            }
        }
        batch.push(IrCommand::SetRootNode { id: NodeId(1) });
        batch
    }

    #[test]
    fn bench_encode_decode_json_vs_flatbuf() {
        let batch = make_bench_batch(500);
        let iterations = 100;

        // JSON encode
        let json_start = std::time::Instant::now();
        let mut json_bytes = Vec::new();
        for _ in 0..iterations {
            json_bytes = encode_batch(&batch).unwrap();
        }
        let json_encode_us = json_start.elapsed().as_micros() / iterations;

        // JSON decode
        let json_decode_start = std::time::Instant::now();
        for _ in 0..iterations {
            let _ = decode_batch(&json_bytes).unwrap();
        }
        let json_decode_us = json_decode_start.elapsed().as_micros() / iterations;

        // FlatBuffers encode
        let fb_start = std::time::Instant::now();
        let mut fb_bytes = Vec::new();
        for _ in 0..iterations {
            fb_bytes = encode_batch_flatbuf(&batch);
        }
        let fb_encode_us = fb_start.elapsed().as_micros() / iterations;

        // FlatBuffers decode
        let fb_decode_start = std::time::Instant::now();
        for _ in 0..iterations {
            let _ = decode_batch_flatbuf(&fb_bytes).unwrap();
        }
        let fb_decode_us = fb_decode_start.elapsed().as_micros() / iterations;

        println!("\n┌──────────────────────────────────────────────────────────┐");
        println!("│  IR Encode/Decode Benchmark (500 nodes, {iterations} iterations)  │");
        println!("├───────────────────┬──────────────┬──────────────────────┤");
        println!("│                   │  JSON (µs)   │  FlatBuffers (µs)    │");
        println!("├───────────────────┼──────────────┼──────────────────────┤");
        println!("│ Encode            │  {json_encode_us:>10}  │  {fb_encode_us:>18}  │");
        println!("│ Decode            │  {json_decode_us:>10}  │  {fb_decode_us:>18}  │");
        println!("├───────────────────┼──────────────┼──────────────────────┤");
        println!("│ Size (bytes)      │  {json_size:>10}  │  {fb_size:>18}  │",
            json_size = json_bytes.len(), fb_size = fb_bytes.len());
        println!("└───────────────────┴──────────────┴──────────────────────┘\n");

        // FlatBuffers should be faster for decoding (zero-copy)
        // We don't assert on speed (CI variability), just that both work
        assert_eq!(decode_batch_flatbuf(&fb_bytes).unwrap().commands.len(),
                   decode_batch(&json_bytes).unwrap().commands.len());
    }
}
