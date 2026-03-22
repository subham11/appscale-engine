//! Shadow Tree — the framework's internal representation of the UI.
//!
//! This mirrors React's fiber tree but lives in Rust.
//! It is the source of truth for:
//! - What nodes exist
//! - Parent/child relationships
//! - Current props for each node
//! - Native view handle mappings

use crate::platform::{NativeHandle, ViewType, PropsDiff, PropValue};
use rustc_hash::FxHashMap;
use std::collections::HashMap;

/// Unique identifier for a node in the shadow tree.
/// Assigned by the reconciler (JavaScript side) and passed through IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "node#{}", self.0)
    }
}

/// A single node in the shadow tree.
#[derive(Debug, Clone)]
pub struct ShadowNode {
    pub id: NodeId,
    pub view_type: ViewType,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub props: HashMap<String, PropValue>,
    pub native_handle: Option<NativeHandle>,

    /// Props that changed since last mount — consumed by mount phase.
    pending_props: PropsDiff,

    /// Component name from React (for DevTools).
    pub component_name: Option<String>,
}

/// The shadow tree — owns all nodes.
pub struct ShadowTree {
    nodes: FxHashMap<NodeId, ShadowNode>,
    root: Option<NodeId>,
}

impl ShadowTree {
    pub fn new() -> Self {
        Self {
            nodes: FxHashMap::default(),
            root: None,
        }
    }

    /// Create a new node. Does NOT insert into the tree hierarchy —
    /// use `append_child` or `insert_before` for that.
    pub fn create_node(
        &mut self,
        id: NodeId,
        view_type: ViewType,
        initial_props: HashMap<String, PropValue>,
    ) {
        let pending = PropsDiff {
            changes: initial_props.clone(),
        };

        let node = ShadowNode {
            id,
            view_type,
            parent: None,
            children: Vec::new(),
            props: initial_props,
            native_handle: None,
            pending_props: pending,
            component_name: None,
        };

        self.nodes.insert(id, node);
    }

    /// Set the root node of the tree.
    pub fn set_root(&mut self, id: NodeId) {
        self.root = Some(id);
    }

    /// Get the root node ID.
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Get a node by ID (immutable).
    pub fn get(&self, id: NodeId) -> Option<&ShadowNode> {
        self.nodes.get(&id)
    }

    /// Get a node by ID (mutable).
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut ShadowNode> {
        self.nodes.get_mut(&id)
    }

    /// Update props on a node. Changed props go into pending_props
    /// for the mount phase to consume.
    pub fn update_props(&mut self, id: NodeId, diff: &PropsDiff) {
        if let Some(node) = self.nodes.get_mut(&id) {
            for (key, value) in &diff.changes {
                node.props.insert(key.clone(), value.clone());
                node.pending_props.set(key.clone(), value.clone());
            }
        }
    }

    /// Append a child to a parent node.
    pub fn append_child(&mut self, parent_id: NodeId, child_id: NodeId) {
        // Set parent reference on child
        if let Some(child) = self.nodes.get_mut(&child_id) {
            child.parent = Some(parent_id);
        }

        // Add to parent's children list
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            if !parent.children.contains(&child_id) {
                parent.children.push(child_id);
            }
        }
    }

    /// Insert a child before another child in a parent's children list.
    pub fn insert_before(
        &mut self,
        parent_id: NodeId,
        child_id: NodeId,
        before_id: NodeId,
    ) {
        if let Some(child) = self.nodes.get_mut(&child_id) {
            child.parent = Some(parent_id);
        }

        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            if let Some(pos) = parent.children.iter().position(|&c| c == before_id) {
                parent.children.insert(pos, child_id);
            } else {
                parent.children.push(child_id);
            }
        }
    }

    /// Remove a child from its parent and the tree.
    pub fn remove_child(&mut self, parent_id: NodeId, child_id: NodeId) {
        // Remove from parent's children list
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children.retain(|&c| c != child_id);
        }

        // Recursively remove the child and all its descendants
        self.remove_subtree(child_id);
    }

    /// Recursively remove a node and all its descendants.
    fn remove_subtree(&mut self, id: NodeId) {
        let children = self.nodes.get(&id)
            .map(|n| n.children.clone())
            .unwrap_or_default();

        for child_id in children {
            self.remove_subtree(child_id);
        }

        self.nodes.remove(&id);
    }

    /// Set the native handle for a node (after platform bridge creates the view).
    pub fn set_native_handle(&mut self, id: NodeId, handle: NativeHandle) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.native_handle = Some(handle);
        }
    }

    /// Take pending props (moves them out, leaving empty).
    /// Called by the mount phase after applying to native view.
    pub fn take_pending_props(&mut self, id: NodeId) -> PropsDiff {
        self.nodes.get_mut(&id)
            .map(|n| std::mem::take(&mut n.pending_props))
            .unwrap_or_default()
    }

    /// Get the ordered children of a node.
    pub fn children_of(&self, id: NodeId) -> &[NodeId] {
        self.nodes.get(&id)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }

    /// Walk ancestors from a node up to root (inclusive).
    /// Returns [root, ..., parent, node_id].
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = Some(id);

        while let Some(nid) = current {
            path.push(nid);
            current = self.nodes.get(&nid).and_then(|n| n.parent);
        }

        path.reverse();
        path
    }

    /// Total number of nodes in the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Iterate all nodes (for DevTools snapshot).
    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &ShadowNode)> {
        self.nodes.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_parent() {
        let mut tree = ShadowTree::new();

        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        tree.create_node(NodeId(2), ViewType::Text, HashMap::new());
        tree.create_node(NodeId(3), ViewType::Text, HashMap::new());

        tree.set_root(NodeId(1));
        tree.append_child(NodeId(1), NodeId(2));
        tree.append_child(NodeId(1), NodeId(3));

        assert_eq!(tree.children_of(NodeId(1)), &[NodeId(2), NodeId(3)]);
        assert_eq!(tree.get(NodeId(2)).unwrap().parent, Some(NodeId(1)));
    }

    #[test]
    fn test_ancestors() {
        let mut tree = ShadowTree::new();

        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        tree.create_node(NodeId(2), ViewType::Container, HashMap::new());
        tree.create_node(NodeId(3), ViewType::Text, HashMap::new());

        tree.set_root(NodeId(1));
        tree.append_child(NodeId(1), NodeId(2));
        tree.append_child(NodeId(2), NodeId(3));

        let path = tree.ancestors(NodeId(3));
        assert_eq!(path, vec![NodeId(1), NodeId(2), NodeId(3)]);
    }

    #[test]
    fn test_remove_subtree() {
        let mut tree = ShadowTree::new();

        tree.create_node(NodeId(1), ViewType::Container, HashMap::new());
        tree.create_node(NodeId(2), ViewType::Container, HashMap::new());
        tree.create_node(NodeId(3), ViewType::Text, HashMap::new());

        tree.append_child(NodeId(1), NodeId(2));
        tree.append_child(NodeId(2), NodeId(3));

        tree.remove_child(NodeId(1), NodeId(2));

        assert_eq!(tree.len(), 1); // Only root remains
        assert!(tree.get(NodeId(2)).is_none());
        assert!(tree.get(NodeId(3)).is_none());
    }
}
