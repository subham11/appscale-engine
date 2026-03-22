//! Layout Engine — Taffy integration for Flexbox + CSS Grid computation.
//!
//! The layout engine owns the Taffy tree and computes absolute positions
//! for every node. It runs on a background thread (layout can be expensive)
//! and produces a LayoutResult that the mount phase consumes.

use crate::platform::PlatformBridge;
use crate::tree::{NodeId, ShadowTree};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use taffy::prelude::*;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Public types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Framework-level layout style (what the developer writes in JSX).
/// This is a subset of CSS that maps cleanly to Taffy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutStyle {
    #[serde(default)]
    pub display: Display,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub flex_direction: FlexDirection,
    #[serde(default)]
    pub flex_wrap: FlexWrap,
    #[serde(default)]
    pub flex_grow: f32,
    #[serde(default = "default_flex_shrink")]
    pub flex_shrink: f32,
    #[serde(default)]
    pub justify_content: Option<JustifyContent>,
    #[serde(default)]
    pub align_items: Option<AlignItems>,
    #[serde(default)]
    pub width: Dimension,
    #[serde(default)]
    pub height: Dimension,
    #[serde(default)]
    pub min_width: Dimension,
    #[serde(default)]
    pub min_height: Dimension,
    #[serde(default)]
    pub max_width: Dimension,
    #[serde(default)]
    pub max_height: Dimension,
    #[serde(default)]
    pub aspect_ratio: Option<f32>,
    #[serde(default)]
    pub margin: Edges,
    #[serde(default)]
    pub padding: Edges,
    #[serde(default)]
    pub gap: f32,
    #[serde(default)]
    pub overflow: Overflow,
}

fn default_flex_shrink() -> f32 {
    1.0
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Display {
    #[default]
    Flex,
    Grid,
    None,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Position {
    #[default]
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum FlexDirection {
    #[default]
    Column,
    Row,
    ColumnReverse,
    RowReverse,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum JustifyContent {
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AlignItems {
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    Baseline,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Dimension {
    #[default]
    Auto,
    Points(f32),
    Percent(f32),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Scroll,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Edges {
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub bottom: f32,
    #[serde(default)]
    pub left: f32,
}

/// Computed layout for a single node — absolute screen coordinates.
#[derive(Debug, Clone, Copy, Default)]
pub struct ComputedLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Layout engine
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct LayoutEngine {
    taffy: TaffyTree<NodeId>,
    node_map: FxHashMap<NodeId, taffy::NodeId>,
    computed: FxHashMap<NodeId, ComputedLayout>,
    root: Option<NodeId>,
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self {
            taffy: TaffyTree::new(),
            node_map: FxHashMap::default(),
            computed: FxHashMap::default(),
            root: None,
        }
    }

    pub fn set_root(&mut self, id: NodeId) {
        self.root = Some(id);
    }

    pub fn create_node(&mut self, id: NodeId, style: &LayoutStyle) -> Result<(), LayoutError> {
        let taffy_style = convert_style(style);
        let taffy_node = self
            .taffy
            .new_leaf_with_context(taffy_style, id)
            .map_err(|e| LayoutError::TaffyError(format!("{e}")))?;
        self.node_map.insert(id, taffy_node);
        Ok(())
    }

    pub fn update_style(&mut self, id: NodeId, style: &LayoutStyle) -> Result<(), LayoutError> {
        let taffy_node = self
            .node_map
            .get(&id)
            .ok_or(LayoutError::NodeNotFound(id))?;
        let taffy_style = convert_style(style);
        self.taffy
            .set_style(*taffy_node, taffy_style)
            .map_err(|e| LayoutError::TaffyError(format!("{e}")))?;
        Ok(())
    }

    pub fn remove_node(&mut self, id: NodeId) {
        if let Some(taffy_node) = self.node_map.remove(&id) {
            let _ = self.taffy.remove(taffy_node);
        }
        self.computed.remove(&id);
    }

    /// Sync children from the shadow tree to the Taffy tree.
    pub fn set_children_from_tree(
        &mut self,
        parent_id: NodeId,
        tree: &ShadowTree,
    ) -> Result<(), LayoutError> {
        let parent_taffy = *self
            .node_map
            .get(&parent_id)
            .ok_or(LayoutError::NodeNotFound(parent_id))?;

        let children: Vec<taffy::NodeId> = tree
            .children_of(parent_id)
            .iter()
            .filter_map(|id| self.node_map.get(id).copied())
            .collect();

        self.taffy
            .set_children(parent_taffy, &children)
            .map_err(|e| LayoutError::TaffyError(format!("{e}")))?;
        Ok(())
    }

    /// Compute layout for the entire tree.
    pub fn compute(
        &mut self,
        tree: &ShadowTree,
        screen_width: f32,
        screen_height: f32,
        platform: &dyn PlatformBridge,
    ) -> Result<(), LayoutError> {
        let root_id = self.root.ok_or(LayoutError::NoRoot)?;
        let root_taffy = *self
            .node_map
            .get(&root_id)
            .ok_or(LayoutError::NodeNotFound(root_id))?;

        let available = taffy::Size {
            width: AvailableSpace::Definite(screen_width),
            height: AvailableSpace::Definite(screen_height),
        };

        // Compute with text measurement callback
        self.taffy
            .compute_layout_with_measure(
                root_taffy,
                available,
                |_known_dims, available_space, _node_id, node_context, _style| {
                    if let Some(framework_id) = node_context {
                        // Check if this is a Text node that needs measurement
                        if let Some(node) = tree.get(*framework_id) {
                            if node.view_type == crate::platform::ViewType::Text {
                                if let Some(crate::platform::PropValue::String(text)) =
                                    node.props.get("text")
                                {
                                    let max_w = match available_space.width {
                                        AvailableSpace::Definite(w) => w,
                                        _ => f32::INFINITY,
                                    };
                                    let metrics = platform.measure_text(
                                        text,
                                        &crate::platform::TextStyle::default(),
                                        max_w,
                                    );
                                    return taffy::Size {
                                        width: metrics.width,
                                        height: metrics.height,
                                    };
                                }
                            }
                        }
                    }
                    taffy::Size::ZERO
                },
            )
            .map_err(|e| LayoutError::TaffyError(format!("{e}")))?;

        // Collect computed layouts with absolute positions
        self.computed.clear();
        self.collect_layouts(root_taffy, 0.0, 0.0);

        Ok(())
    }

    fn collect_layouts(&mut self, node: taffy::NodeId, parent_x: f32, parent_y: f32) {
        let layout = self.taffy.layout(node).unwrap();
        let abs_x = parent_x + layout.location.x;
        let abs_y = parent_y + layout.location.y;

        let framework_id = self.taffy.get_node_context(node).copied();
        if let Some(fid) = framework_id {
            self.computed.insert(
                fid,
                ComputedLayout {
                    x: abs_x,
                    y: abs_y,
                    width: layout.size.width,
                    height: layout.size.height,
                },
            );
        }

        let children = self.taffy.children(node).unwrap();
        for &child in &children {
            self.collect_layouts(child, abs_x, abs_y);
        }
    }

    /// Get computed layout for a node.
    pub fn get_computed(&self, id: NodeId) -> Option<&ComputedLayout> {
        self.computed.get(&id)
    }

    /// Hit test: find nodes at a given screen coordinate.
    /// Returns nodes sorted by specificity (smallest area first).
    pub fn hit_test(&self, x: f32, y: f32) -> Vec<NodeId> {
        let mut hits: Vec<(NodeId, f32)> = self
            .computed
            .iter()
            .filter(|(_, layout)| {
                x >= layout.x
                    && x <= layout.x + layout.width
                    && y >= layout.y
                    && y <= layout.y + layout.height
            })
            .map(|(&id, layout)| (id, layout.width * layout.height))
            .collect();

        hits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        hits.into_iter().map(|(id, _)| id).collect()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Style conversion
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn convert_style(style: &LayoutStyle) -> taffy::Style {
    taffy::Style {
        display: match style.display {
            Display::Flex => taffy::Display::Flex,
            Display::Grid => taffy::Display::Grid,
            Display::None => taffy::Display::None,
        },
        position: match style.position {
            Position::Relative => taffy::Position::Relative,
            Position::Absolute => taffy::Position::Absolute,
        },
        flex_direction: match style.flex_direction {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::Column => taffy::FlexDirection::Column,
            FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
            FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
        },
        flex_wrap: match style.flex_wrap {
            FlexWrap::NoWrap => taffy::FlexWrap::NoWrap,
            FlexWrap::Wrap => taffy::FlexWrap::Wrap,
            FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        },
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        justify_content: style.justify_content.map(|j| match j {
            JustifyContent::FlexStart => taffy::JustifyContent::FlexStart,
            JustifyContent::FlexEnd => taffy::JustifyContent::FlexEnd,
            JustifyContent::Center => taffy::JustifyContent::Center,
            JustifyContent::SpaceBetween => taffy::JustifyContent::SpaceBetween,
            JustifyContent::SpaceAround => taffy::JustifyContent::SpaceAround,
            JustifyContent::SpaceEvenly => taffy::JustifyContent::SpaceEvenly,
        }),
        align_items: style.align_items.map(|a| match a {
            AlignItems::FlexStart => taffy::AlignItems::FlexStart,
            AlignItems::FlexEnd => taffy::AlignItems::FlexEnd,
            AlignItems::Center => taffy::AlignItems::Center,
            AlignItems::Stretch => taffy::AlignItems::Stretch,
            AlignItems::Baseline => taffy::AlignItems::Baseline,
        }),
        size: taffy::Size {
            width: convert_dimension(style.width),
            height: convert_dimension(style.height),
        },
        min_size: taffy::Size {
            width: convert_dimension(style.min_width),
            height: convert_dimension(style.min_height),
        },
        max_size: taffy::Size {
            width: convert_dimension(style.max_width),
            height: convert_dimension(style.max_height),
        },
        aspect_ratio: style.aspect_ratio,
        margin: taffy::Rect {
            top: length(style.margin.top),
            right: length(style.margin.right),
            bottom: length(style.margin.bottom),
            left: length(style.margin.left),
        },
        padding: taffy::Rect {
            top: length_padding(style.padding.top),
            right: length_padding(style.padding.right),
            bottom: length_padding(style.padding.bottom),
            left: length_padding(style.padding.left),
        },
        gap: taffy::Size {
            width: taffy::LengthPercentage::Length(style.gap),
            height: taffy::LengthPercentage::Length(style.gap),
        },
        overflow: taffy::Point {
            x: convert_overflow(style.overflow),
            y: convert_overflow(style.overflow),
        },
        ..Default::default()
    }
}

fn convert_dimension(dim: Dimension) -> taffy::Dimension {
    match dim {
        Dimension::Auto => taffy::Dimension::Auto,
        Dimension::Points(v) => taffy::Dimension::Length(v),
        Dimension::Percent(v) => taffy::Dimension::Percent(v / 100.0),
    }
}

fn length(v: f32) -> taffy::LengthPercentageAuto {
    taffy::LengthPercentageAuto::Length(v)
}

fn length_padding(v: f32) -> taffy::LengthPercentage {
    taffy::LengthPercentage::Length(v)
}

fn convert_overflow(o: Overflow) -> taffy::Overflow {
    match o {
        Overflow::Visible => taffy::Overflow::Visible,
        Overflow::Hidden => taffy::Overflow::Hidden,
        Overflow::Scroll => taffy::Overflow::Scroll,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Errors
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("Taffy error: {0}")]
    TaffyError(String),

    #[error("Node not found: {0}")]
    NodeNotFound(NodeId),

    #[error("No root node set")]
    NoRoot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::mock::MockPlatform;
    use crate::platform::ViewType;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_basic_layout() {
        let platform = Arc::new(MockPlatform::new());
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();

        // Create: root (flex column) → child (100x50)
        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        tree.create_node(NodeId(2), ViewType::Container, HashMap::new());
        tree.set_root(NodeId(1));
        tree.append_child(NodeId(1), NodeId(2));

        layout
            .create_node(
                NodeId(1),
                &LayoutStyle {
                    width: Dimension::Points(390.0),
                    height: Dimension::Points(844.0),
                    ..Default::default()
                },
            )
            .unwrap();

        layout
            .create_node(
                NodeId(2),
                &LayoutStyle {
                    width: Dimension::Points(100.0),
                    height: Dimension::Points(50.0),
                    ..Default::default()
                },
            )
            .unwrap();

        layout.set_root(NodeId(1));
        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.compute(&tree, 390.0, 844.0, &*platform).unwrap();

        let root_layout = layout.get_computed(NodeId(1)).unwrap();
        assert_eq!(root_layout.width, 390.0);
        assert_eq!(root_layout.height, 844.0);

        let child_layout = layout.get_computed(NodeId(2)).unwrap();
        assert_eq!(child_layout.width, 100.0);
        assert_eq!(child_layout.height, 50.0);
        assert_eq!(child_layout.x, 0.0);
        assert_eq!(child_layout.y, 0.0);
    }

    #[test]
    fn stress_deep_nesting() {
        // 50-level deep nested tree — exercises Taffy's recursive layout
        let platform = Arc::new(MockPlatform::new());
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();
        let depth = 50;

        for i in 1..=(depth as u64) {
            tree.create_node(NodeId(i), ViewType::Container, HashMap::new());
            let style = if i == 1 {
                LayoutStyle {
                    width: Dimension::Points(400.0),
                    height: Dimension::Points(800.0),
                    ..Default::default()
                }
            } else {
                LayoutStyle {
                    flex_grow: 1.0,
                    ..Default::default()
                }
            };
            layout.create_node(NodeId(i), &style).unwrap();
        }

        tree.set_root(NodeId(1));
        layout.set_root(NodeId(1));

        for i in 1..(depth as u64) {
            tree.append_child(NodeId(i), NodeId(i + 1));
            layout.set_children_from_tree(NodeId(i), &tree).unwrap();
        }

        layout.compute(&tree, 400.0, 800.0, &*platform).unwrap();

        // Root should be full size
        let root = layout.get_computed(NodeId(1)).unwrap();
        assert_eq!(root.width, 400.0);
        assert_eq!(root.height, 800.0);

        // Deepest node should still have computed layout
        let deepest = layout.get_computed(NodeId(depth as u64)).unwrap();
        assert!(
            deepest.width > 0.0,
            "Deepest node should have non-zero width"
        );
        assert!(
            deepest.height > 0.0,
            "Deepest node should have non-zero height"
        );
    }

    #[test]
    fn stress_wide_tree() {
        // 200 children in a single flex row container
        let platform = Arc::new(MockPlatform::new());
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();
        let child_count = 200u64;

        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(1),
                &LayoutStyle {
                    width: Dimension::Points(1000.0),
                    height: Dimension::Points(100.0),
                    flex_direction: FlexDirection::Row,
                    ..Default::default()
                },
            )
            .unwrap();

        tree.set_root(NodeId(1));
        layout.set_root(NodeId(1));

        for i in 2..=(child_count + 1) {
            tree.create_node(NodeId(i), ViewType::Container, HashMap::new());
            layout
                .create_node(
                    NodeId(i),
                    &LayoutStyle {
                        width: Dimension::Points(5.0),
                        height: Dimension::Points(20.0),
                        ..Default::default()
                    },
                )
                .unwrap();
            tree.append_child(NodeId(1), NodeId(i));
        }

        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.compute(&tree, 1000.0, 100.0, &*platform).unwrap();

        // All children should be laid out
        for i in 2..=(child_count + 1) {
            let cl = layout.get_computed(NodeId(i));
            assert!(cl.is_some(), "Child {} should have computed layout", i);
        }

        // Children should be positioned left-to-right
        let first = layout.get_computed(NodeId(2)).unwrap();
        let second = layout.get_computed(NodeId(3)).unwrap();
        assert!(
            second.x > first.x,
            "Second child should be to the right of first"
        );
    }

    #[test]
    fn stress_rapid_mutations() {
        // Create nodes, update styles, remove and recreate — simulates heavy reconciliation
        let platform = Arc::new(MockPlatform::new());
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();

        // Initial tree: root with 10 children
        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(1),
                &LayoutStyle {
                    width: Dimension::Points(400.0),
                    height: Dimension::Points(800.0),
                    ..Default::default()
                },
            )
            .unwrap();
        tree.set_root(NodeId(1));
        layout.set_root(NodeId(1));

        for i in 2..=11u64 {
            tree.create_node(NodeId(i), ViewType::Container, HashMap::new());
            layout
                .create_node(
                    NodeId(i),
                    &LayoutStyle {
                        height: Dimension::Points(50.0),
                        ..Default::default()
                    },
                )
                .unwrap();
            tree.append_child(NodeId(1), NodeId(i));
        }
        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.compute(&tree, 400.0, 800.0, &*platform).unwrap();
        assert_eq!(tree.len(), 11);

        // Remove half the children
        for i in (7..=11u64).rev() {
            tree.remove_child(NodeId(1), NodeId(i));
            layout.remove_node(NodeId(i));
        }
        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.compute(&tree, 400.0, 800.0, &*platform).unwrap();
        assert_eq!(tree.len(), 6);

        // Add new children with different styles
        for i in 100..=104u64 {
            tree.create_node(NodeId(i), ViewType::Container, HashMap::new());
            layout
                .create_node(
                    NodeId(i),
                    &LayoutStyle {
                        height: Dimension::Points(30.0),
                        ..Default::default()
                    },
                )
                .unwrap();
            tree.append_child(NodeId(1), NodeId(i));
        }
        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.compute(&tree, 400.0, 800.0, &*platform).unwrap();
        assert_eq!(tree.len(), 11);

        // All current children should have layouts
        for &id in tree.children_of(NodeId(1)) {
            assert!(layout.get_computed(id).is_some());
        }
    }

    #[test]
    fn stress_mixed_dimensions() {
        // Mix of fixed, percentage, and auto dimensions
        let platform = Arc::new(MockPlatform::new());
        let mut tree = ShadowTree::new();
        let mut layout = LayoutEngine::new();

        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(1),
                &LayoutStyle {
                    width: Dimension::Points(400.0),
                    height: Dimension::Points(600.0),
                    ..Default::default()
                },
            )
            .unwrap();
        tree.set_root(NodeId(1));
        layout.set_root(NodeId(1));

        // Child 1: fixed 100x100
        tree.create_node(NodeId(2), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(2),
                &LayoutStyle {
                    width: Dimension::Points(100.0),
                    height: Dimension::Points(100.0),
                    ..Default::default()
                },
            )
            .unwrap();
        tree.append_child(NodeId(1), NodeId(2));

        // Child 2: 50% width, fixed height
        tree.create_node(NodeId(3), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(3),
                &LayoutStyle {
                    width: Dimension::Percent(50.0),
                    height: Dimension::Points(80.0),
                    ..Default::default()
                },
            )
            .unwrap();
        tree.append_child(NodeId(1), NodeId(3));

        // Child 3: flex-grow
        tree.create_node(NodeId(4), ViewType::Container, HashMap::new());
        layout
            .create_node(
                NodeId(4),
                &LayoutStyle {
                    flex_grow: 1.0,
                    ..Default::default()
                },
            )
            .unwrap();
        tree.append_child(NodeId(1), NodeId(4));

        layout.set_children_from_tree(NodeId(1), &tree).unwrap();
        layout.compute(&tree, 400.0, 600.0, &*platform).unwrap();

        let c1 = layout.get_computed(NodeId(2)).unwrap();
        assert_eq!(c1.width, 100.0);
        assert_eq!(c1.height, 100.0);

        let c2 = layout.get_computed(NodeId(3)).unwrap();
        assert_eq!(c2.width, 200.0); // 50% of 400
        assert_eq!(c2.height, 80.0);

        let c3 = layout.get_computed(NodeId(4)).unwrap();
        assert!(c3.height > 0.0, "Flex-grow child should expand");
    }
}
