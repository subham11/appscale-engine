//! AI Layer — IR generation, layout optimization, and training data.
//!
//! This module enables three AI-driven capabilities:
//! 1. **IR Generation**: AI models produce Binary IR directly, bypassing React
//!    reconciler when appropriate (deterministic IR makes this possible)
//! 2. **Layout Optimization**: Analyze layout trees and suggest performance
//!    improvements (e.g., flatten unnecessary nesting, merge redundant nodes)
//! 3. **IR Replay for Training**: Export recorded IR sessions as structured
//!    training data for fine-tuning layout/UI generation models
//!
//! The AI layer reads from `ir.rs` types, `devtools::IrRecorder`, and
//! `layout::LayoutEngine` — it does NOT modify the core engine loop.

use crate::ir::{IrBatch, IrCommand};
use crate::layout::LayoutEngine;
use crate::platform::ViewType;
use crate::tree::{NodeId, ShadowTree};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// IR Generation from AI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A prompt-to-IR generation request.
/// AI models receive this context and produce an `IrBatch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrGenerationRequest {
    /// Natural language description of the desired UI.
    pub prompt: String,

    /// Target platform (affects layout defaults and capabilities).
    pub platform: String,

    /// Screen dimensions for layout constraint.
    pub screen_width: f32,
    pub screen_height: f32,

    /// Optional: existing tree snapshot for incremental updates.
    pub existing_node_count: Option<u32>,

    /// Optional: component palette to constrain generation.
    pub allowed_components: Option<Vec<String>>,
}

/// Result of AI IR generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrGenerationResult {
    /// The generated IR batch ready to apply.
    pub batch: IrBatch,

    /// Confidence score (0.0 – 1.0) from the model.
    pub confidence: f32,

    /// Warnings or notes from the generation process.
    pub warnings: Vec<String>,

    /// Token/node count stats.
    pub stats: GenerationStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationStats {
    pub nodes_created: u32,
    pub commands_generated: u32,
    pub generation_time_ms: f64,
}

/// Validates that a generated IR batch is well-formed.
/// Checks: node IDs are unique, parent references are valid, required props present.
pub fn validate_generated_batch(batch: &IrBatch) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut created_ids: HashMap<u64, usize> = HashMap::new();
    let mut parented_ids: Vec<u64> = Vec::new();

    for (idx, cmd) in batch.commands.iter().enumerate() {
        match cmd {
            IrCommand::CreateNode { id, view_type, .. } => {
                if let Some(&prev_idx) = created_ids.get(&id.0) {
                    issues.push(ValidationIssue {
                        command_index: idx,
                        severity: IssueSeverity::Error,
                        message: format!(
                            "Duplicate node ID {} (first created at command {})",
                            id.0, prev_idx
                        ),
                    });
                }
                created_ids.insert(id.0, idx);

                // Text nodes should have a "text" or "content" prop
                if matches!(view_type, ViewType::Text) {
                    // Warn only — not blocking
                    if let IrCommand::CreateNode { props, .. } = cmd {
                        if !props.contains_key("text") && !props.contains_key("content") {
                            issues.push(ValidationIssue {
                                command_index: idx,
                                severity: IssueSeverity::Warning,
                                message: format!(
                                    "Text node {} has no 'text' or 'content' prop",
                                    id.0
                                ),
                            });
                        }
                    }
                }
            }
            IrCommand::AppendChild { parent, child } => {
                if !created_ids.contains_key(&parent.0) {
                    issues.push(ValidationIssue {
                        command_index: idx,
                        severity: IssueSeverity::Error,
                        message: format!("AppendChild references unknown parent {}", parent.0),
                    });
                }
                if !created_ids.contains_key(&child.0) {
                    issues.push(ValidationIssue {
                        command_index: idx,
                        severity: IssueSeverity::Error,
                        message: format!("AppendChild references unknown child {}", child.0),
                    });
                }
                parented_ids.push(child.0);
            }
            IrCommand::SetRootNode { id } => {
                if !created_ids.contains_key(&id.0) {
                    issues.push(ValidationIssue {
                        command_index: idx,
                        severity: IssueSeverity::Error,
                        message: format!("SetRootNode references unknown node {}", id.0),
                    });
                }
            }
            _ => {}
        }
    }

    // Warn about orphan nodes (created but never parented and not root)
    let has_root = batch
        .commands
        .iter()
        .any(|c| matches!(c, IrCommand::SetRootNode { .. }));
    if has_root {
        for &id in created_ids.keys() {
            let is_root = batch
                .commands
                .iter()
                .any(|c| matches!(c, IrCommand::SetRootNode { id: r } if r.0 == id));
            if !is_root && !parented_ids.contains(&id) {
                issues.push(ValidationIssue {
                    command_index: 0,
                    severity: IssueSeverity::Warning,
                    message: format!("Node {} is created but never attached to the tree", id),
                });
            }
        }
    }

    issues
}

/// A validation issue found in a generated IR batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub command_index: usize,
    pub severity: IssueSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    Warning,
    Error,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Layout Optimization
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A layout optimization hint for a specific node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutHint {
    pub node_id: u64,
    pub hint_type: LayoutHintType,
    pub description: String,
    /// Estimated performance impact (0.0 – 1.0, higher = more impactful).
    pub impact: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayoutHintType {
    /// Node wraps a single child with no visual changes — can be flattened.
    UnnecessaryWrapper,
    /// Deep nesting that could be restructured with flexbox.
    DeepNesting { depth: u32 },
    /// Node has fixed dimensions but uses flex — simplify.
    OverconstrainedLayout,
    /// Sibling nodes have identical styles — consider shared component.
    DuplicateStyles { sibling_count: u32 },
    /// Large flat list without recycling hints.
    UnoptimizedList { child_count: u32 },
}

/// Analyze a shadow tree and produce layout optimization hints.
pub fn analyze_layout(tree: &ShadowTree, _layout: &LayoutEngine) -> Vec<LayoutHint> {
    let mut hints = Vec::new();

    let root = match tree.root() {
        Some(r) => r,
        None => return hints,
    };

    // Analysis passes
    detect_unnecessary_wrappers(root, tree, &mut hints);
    detect_deep_nesting(root, tree, 0, &mut hints);
    detect_large_flat_lists(root, tree, &mut hints);

    // Sort by impact (highest first)
    hints.sort_by(|a, b| {
        b.impact
            .partial_cmp(&a.impact)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hints
}

fn detect_unnecessary_wrappers(node_id: NodeId, tree: &ShadowTree, hints: &mut Vec<LayoutHint>) {
    let node = match tree.get(node_id) {
        Some(n) => n,
        None => return,
    };

    // A wrapper is unnecessary if:
    // - It's a View with exactly 1 child
    // - It has no event handlers (checked via empty props as proxy)
    // - It's not the root
    if matches!(node.view_type, ViewType::Container)
        && node.children.len() == 1
        && node.props.is_empty()
        && tree.root() != Some(node_id)
    {
        hints.push(LayoutHint {
            node_id: node_id.0,
            hint_type: LayoutHintType::UnnecessaryWrapper,
            description: format!(
                "View node {} wraps a single child with no props — consider removing",
                node_id.0
            ),
            impact: 0.3,
        });
    }

    for &child in &node.children {
        detect_unnecessary_wrappers(child, tree, hints);
    }
}

fn detect_deep_nesting(
    node_id: NodeId,
    tree: &ShadowTree,
    depth: u32,
    hints: &mut Vec<LayoutHint>,
) {
    const DEEP_THRESHOLD: u32 = 10;

    if depth >= DEEP_THRESHOLD {
        hints.push(LayoutHint {
            node_id: node_id.0,
            hint_type: LayoutHintType::DeepNesting { depth },
            description: format!(
                "Node {} is nested {} levels deep — consider flattening with flexbox",
                node_id.0, depth
            ),
            impact: 0.6,
        });
        return; // Don't report children — the hint covers the subtree
    }

    let node = match tree.get(node_id) {
        Some(n) => n,
        None => return,
    };

    for &child in &node.children {
        detect_deep_nesting(child, tree, depth + 1, hints);
    }
}

fn detect_large_flat_lists(node_id: NodeId, tree: &ShadowTree, hints: &mut Vec<LayoutHint>) {
    const LARGE_LIST_THRESHOLD: usize = 50;

    let node = match tree.get(node_id) {
        Some(n) => n,
        None => return,
    };

    // A ScrollView or View with many direct children
    if (matches!(node.view_type, ViewType::ScrollView)
        || matches!(node.view_type, ViewType::Container))
        && node.children.len() > LARGE_LIST_THRESHOLD
    {
        hints.push(LayoutHint {
            node_id: node_id.0,
            hint_type: LayoutHintType::UnoptimizedList {
                child_count: node.children.len() as u32,
            },
            description: format!(
                "{:?} node {} has {} children — consider FlatList with recycling",
                node.view_type,
                node_id.0,
                node.children.len()
            ),
            impact: 0.8,
        });
    }

    for &child in &node.children {
        detect_large_flat_lists(child, tree, hints);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// IR Replay for Training Data
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A training data record: an IR session annotated with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRecord {
    /// Unique session ID.
    pub session_id: String,

    /// Platform this session was recorded on.
    pub platform: String,

    /// Screen dimensions at recording time.
    pub screen_width: f32,
    pub screen_height: f32,

    /// Sequence of IR batches (from `IrRecorder`).
    pub batches: Vec<TrainingBatch>,

    /// Optional: final tree structure for supervised learning.
    pub final_tree: Option<TreeSnapshot>,

    /// Annotation tags (e.g., "login_flow", "settings_page").
    pub tags: Vec<String>,
}

/// An IR batch within a training record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingBatch {
    /// Relative timestamp within the session (ms).
    pub offset_ms: f64,

    /// The IR batch.
    pub batch: IrBatch,

    /// Optional annotation for this specific batch.
    pub annotation: Option<String>,
}

/// Simplified tree snapshot for training data export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSnapshot {
    pub nodes: Vec<TreeNodeSnapshot>,
    pub root_id: Option<u64>,
}

/// A single node in a training data tree snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNodeSnapshot {
    pub id: u64,
    pub view_type: String,
    pub children: Vec<u64>,
    pub prop_count: u32,
    pub depth: u32,
}

/// Export an `IrRecorder`'s recorded batches as a training record.
pub fn export_training_record(
    session_id: &str,
    platform: &str,
    screen_width: f32,
    screen_height: f32,
    recorded_batches: &[crate::devtools::TimestampedBatch],
    tree: Option<&ShadowTree>,
    tags: Vec<String>,
) -> TrainingRecord {
    let batches = recorded_batches
        .iter()
        .map(|tb| TrainingBatch {
            offset_ms: tb.offset_ms,
            batch: tb.batch.clone(),
            annotation: None,
        })
        .collect();

    let final_tree = tree.map(snapshot_tree_for_training);

    TrainingRecord {
        session_id: session_id.to_string(),
        platform: platform.to_string(),
        screen_width,
        screen_height,
        batches,
        final_tree,
        tags,
    }
}

fn snapshot_tree_for_training(tree: &ShadowTree) -> TreeSnapshot {
    let mut nodes = Vec::new();
    let root_id = tree.root().map(|r| r.0);

    if let Some(root) = tree.root() {
        snapshot_node_recursive(root, tree, 0, &mut nodes);
    }

    TreeSnapshot { nodes, root_id }
}

fn snapshot_node_recursive(
    node_id: NodeId,
    tree: &ShadowTree,
    depth: u32,
    nodes: &mut Vec<TreeNodeSnapshot>,
) {
    let node = match tree.get(node_id) {
        Some(n) => n,
        None => return,
    };

    let children_ids: Vec<u64> = node.children.iter().map(|c| c.0).collect();

    nodes.push(TreeNodeSnapshot {
        id: node_id.0,
        view_type: format!("{:?}", node.view_type),
        children: children_ids.clone(),
        prop_count: node.props.len() as u32,
        depth,
    });

    for &child in &node.children {
        snapshot_node_recursive(child, tree, depth + 1, nodes);
    }
}

/// Aggregate statistics about a training record for model analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingStats {
    pub total_batches: usize,
    pub total_commands: usize,
    pub command_histogram: HashMap<String, usize>,
    pub session_duration_ms: f64,
    pub avg_batch_size: f64,
    pub unique_node_ids: usize,
}

/// Compute statistics from a training record.
pub fn compute_training_stats(record: &TrainingRecord) -> TrainingStats {
    let total_batches = record.batches.len();
    let mut total_commands = 0;
    let mut histogram: HashMap<String, usize> = HashMap::new();
    let mut node_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut max_offset = 0.0_f64;

    for tb in &record.batches {
        total_commands += tb.batch.commands.len();
        if tb.offset_ms > max_offset {
            max_offset = tb.offset_ms;
        }

        for cmd in &tb.batch.commands {
            let cmd_type = match cmd {
                IrCommand::CreateNode { id, .. } => {
                    node_ids.insert(id.0);
                    "CreateNode"
                }
                IrCommand::UpdateProps { id, .. } => {
                    node_ids.insert(id.0);
                    "UpdateProps"
                }
                IrCommand::UpdateStyle { id, .. } => {
                    node_ids.insert(id.0);
                    "UpdateStyle"
                }
                IrCommand::AppendChild { parent, child } => {
                    node_ids.insert(parent.0);
                    node_ids.insert(child.0);
                    "AppendChild"
                }
                IrCommand::InsertBefore { .. } => "InsertBefore",
                IrCommand::RemoveChild { .. } => "RemoveChild",
                IrCommand::SetRootNode { id } => {
                    node_ids.insert(id.0);
                    "SetRootNode"
                }
            };
            *histogram.entry(cmd_type.to_string()).or_insert(0) += 1;
        }
    }

    TrainingStats {
        total_batches,
        total_commands,
        command_histogram: histogram,
        session_duration_ms: max_offset,
        avg_batch_size: if total_batches > 0 {
            total_commands as f64 / total_batches as f64
        } else {
            0.0
        },
        unique_node_ids: node_ids.len(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IrBatch;
    use crate::layout::LayoutStyle;
    use crate::platform::PropValue;

    fn make_batch() -> IrBatch {
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
        batch
    }

    #[test]
    fn validate_well_formed_batch() {
        let batch = make_batch();
        let issues = validate_generated_batch(&batch);
        let errors: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "Expected no errors: {:?}", errors);
    }

    #[test]
    fn validate_duplicate_ids() {
        let mut batch = IrBatch::new(1);
        batch.push(IrCommand::CreateNode {
            id: NodeId(1),
            view_type: ViewType::Container,
            props: HashMap::new(),
            style: LayoutStyle::default(),
        });
        batch.push(IrCommand::CreateNode {
            id: NodeId(1), // duplicate!
            view_type: ViewType::Text,
            props: HashMap::new(),
            style: LayoutStyle::default(),
        });

        let issues = validate_generated_batch(&batch);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error && i.message.contains("Duplicate")));
    }

    #[test]
    fn validate_unknown_parent() {
        let mut batch = IrBatch::new(1);
        batch.push(IrCommand::CreateNode {
            id: NodeId(1),
            view_type: ViewType::Container,
            props: HashMap::new(),
            style: LayoutStyle::default(),
        });
        batch.push(IrCommand::AppendChild {
            parent: NodeId(99), // unknown
            child: NodeId(1),
        });

        let issues = validate_generated_batch(&batch);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error && i.message.contains("unknown parent")));
    }

    #[test]
    fn validate_text_node_warning() {
        let mut batch = IrBatch::new(1);
        batch.push(IrCommand::CreateNode {
            id: NodeId(1),
            view_type: ViewType::Text,
            props: HashMap::new(), // no "text" prop
            style: LayoutStyle::default(),
        });

        let issues = validate_generated_batch(&batch);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Warning && i.message.contains("no 'text'")));
    }

    #[test]
    fn layout_analysis_empty_tree() {
        let tree = ShadowTree::new();
        let layout = LayoutEngine::new();
        let hints = analyze_layout(&tree, &layout);
        assert!(hints.is_empty());
    }

    #[test]
    fn training_record_export() {
        let batch = make_batch();
        let timestamped = vec![
            crate::devtools::TimestampedBatch {
                offset_ms: 0.0,
                batch: batch.clone(),
            },
            crate::devtools::TimestampedBatch {
                offset_ms: 16.6,
                batch: batch.clone(),
            },
        ];

        let record = export_training_record(
            "session-001",
            "ios",
            390.0,
            844.0,
            &timestamped,
            None,
            vec!["test".to_string()],
        );

        assert_eq!(record.session_id, "session-001");
        assert_eq!(record.platform, "ios");
        assert_eq!(record.batches.len(), 2);
        assert_eq!(record.tags, vec!["test"]);
        assert!(record.final_tree.is_none());
    }

    #[test]
    fn training_stats_computation() {
        let batch = make_batch();
        let record = TrainingRecord {
            session_id: "s1".to_string(),
            platform: "web".to_string(),
            screen_width: 1920.0,
            screen_height: 1080.0,
            batches: vec![
                TrainingBatch {
                    offset_ms: 0.0,
                    batch: batch.clone(),
                    annotation: None,
                },
                TrainingBatch {
                    offset_ms: 16.6,
                    batch: batch.clone(),
                    annotation: None,
                },
            ],
            final_tree: None,
            tags: vec![],
        };

        let stats = compute_training_stats(&record);
        assert_eq!(stats.total_batches, 2);
        assert_eq!(stats.total_commands, 8); // 4 cmds × 2 batches
        assert_eq!(stats.unique_node_ids, 2); // nodes 1 and 2
        assert!((stats.avg_batch_size - 4.0).abs() < 0.001);
        assert!((stats.session_duration_ms - 16.6).abs() < 0.001);
        assert_eq!(*stats.command_histogram.get("CreateNode").unwrap(), 4);
        assert_eq!(*stats.command_histogram.get("AppendChild").unwrap(), 2);
        assert_eq!(*stats.command_histogram.get("SetRootNode").unwrap(), 2);
    }

    #[test]
    fn ir_generation_request_serialization() {
        let req = IrGenerationRequest {
            prompt: "Create a login form".to_string(),
            platform: "ios".to_string(),
            screen_width: 390.0,
            screen_height: 844.0,
            existing_node_count: None,
            allowed_components: Some(vec![
                "View".to_string(),
                "TextInput".to_string(),
                "Button".to_string(),
            ]),
        };

        let json = serde_json::to_string(&req).unwrap();
        let decoded: IrGenerationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.prompt, "Create a login form");
        assert_eq!(decoded.allowed_components.unwrap().len(), 3);
    }

    #[test]
    fn validation_issue_serialization() {
        let issue = ValidationIssue {
            command_index: 5,
            severity: IssueSeverity::Error,
            message: "test error".to_string(),
        };

        let json = serde_json::to_string(&issue).unwrap();
        let decoded: ValidationIssue = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command_index, 5);
        assert_eq!(decoded.severity, IssueSeverity::Error);
    }
}
