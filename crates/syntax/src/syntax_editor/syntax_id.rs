use la_arena::{Arena, Idx};
use rustc_hash::FxHashMap;

use crate::{SyntaxNode, SyntaxNodePtr};

/// Stable identifier for a SyntaxNode that survives cloning.
///
/// This provides identity that is independent of reference counting,
/// allowing multiple SyntaxNode instances to share the same identity
/// if they represent the same logical position in the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxId(Idx<()>);

/// Manages bidirectional mapping between SyntaxNodes and SyntaxIds.
///
/// Uses position-based lookup (via SyntaxNodePtr) to ensure that
/// cloned nodes at the same position resolve to the same ID.
#[derive(Debug, Default)]
pub struct SyntaxIdMap {
    arena: Arena<()>,
    /// Position-based lookup: ensures cloned nodes map to same ID
    node_to_id: FxHashMap<SyntaxNodePtr, SyntaxId>,
    /// Store representative node for each ID
    id_to_node: FxHashMap<SyntaxId, SyntaxNode>,
}

impl SyntaxIdMap {
    /// Allocate a new SyntaxId for the given node, or return existing ID if already allocated.
    ///
    /// Uses SyntaxNodePtr for lookup, so cloned nodes at the same position
    /// will receive the same ID.
    pub fn alloc(&mut self, node: &SyntaxNode) -> SyntaxId {
        let ptr = SyntaxNodePtr::new(node);

        if let Some(&id) = self.node_to_id.get(&ptr) {
            return id;
        }

        let id = SyntaxId(self.arena.alloc(()));
        self.node_to_id.insert(ptr, id);
        self.id_to_node.insert(id, node.clone());
        id
    }

    /// Get the SyntaxId for a node, if it has been allocated.
    pub fn get_id(&self, node: &SyntaxNode) -> Option<SyntaxId> {
        let ptr = SyntaxNodePtr::new(node);
        self.node_to_id.get(&ptr).copied()
    }

    /// Get the representative node for a SyntaxId.
    pub fn get_node(&self, id: SyntaxId) -> Option<&SyntaxNode> {
        self.id_to_node.get(&id)
    }

    /// Merge another SyntaxIdMap into this one, returning a remapping table.
    ///
    /// The remapping table maps old IDs from `other` to new IDs in `self`.
    /// This is necessary because the same node position might already have
    /// an ID allocated in `self`.
    pub fn merge(&mut self, other: SyntaxIdMap) -> FxHashMap<SyntaxId, SyntaxId> {
        let mut id_remapping = FxHashMap::default();

        for (_ptr, old_id) in other.node_to_id {
            let node = other.id_to_node.get(&old_id).expect("node should exist for allocated ID");
            let new_id = self.alloc(node);
            id_remapping.insert(old_id, new_id);
        }

        id_remapping
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::make;
    use crate::AstNode;

    #[test]
    fn test_basic_allocation_and_lookup() {
        let mut map = SyntaxIdMap::default();

        let node = make::item_const(
            [],
            None,
            make::name("FOO"),
            make::ty("i32"),
            make::expr_literal("42").into(),
        )
        .syntax()
        .clone_for_update();

        let id = map.alloc(&node);

        // Should be able to retrieve the same ID
        assert_eq!(map.get_id(&node), Some(id));

        // Should be able to retrieve the node
        assert_eq!(map.get_node(id).unwrap(), &node);
    }

    #[test]
    fn test_clone_stability() {
        let mut map = SyntaxIdMap::default();

        let node = make::item_const(
            [],
            None,
            make::name("FOO"),
            make::ty("i32"),
            make::expr_literal("42").into(),
        )
        .syntax()
        .clone_for_update();

        let id1 = map.alloc(&node);

        // Clone the node - should get the same ID since it's at the same position
        let cloned_node = node.clone();
        let id2 = map.alloc(&cloned_node);

        assert_eq!(id1, id2, "Cloned node should have same ID");
        assert_eq!(map.get_id(&cloned_node), Some(id1));
    }

    #[test]
    fn test_different_nodes_get_different_ids() {
        let mut map = SyntaxIdMap::default();

        // Create a parent containing multiple children so they have different positions
        let item_list = make::item_list(Some(vec![
            make::item_const(
                [],
                None,
                make::name("FOO"),
                make::ty("i32"),
                make::expr_literal("42").into(),
            )
            .into(),
            make::item_const(
                [],
                None,
                make::name("BAR"),
                make::ty("i32"),
                make::expr_literal("43").into(),
            )
            .into(),
        ]))
        .clone_for_update();

        let items: Vec<_> = item_list.syntax().children().collect();
        let node1 = &items[0];
        let node2 = &items[1];

        let id1 = map.alloc(node1);
        let id2 = map.alloc(node2);

        assert_ne!(id1, id2, "Different nodes should have different IDs");
    }

    #[test]
    fn test_merge_with_remapping() {
        let mut map1 = SyntaxIdMap::default();
        let mut map2 = SyntaxIdMap::default();

        let node1 = make::item_const(
            [],
            None,
            make::name("FOO"),
            make::ty("i32"),
            make::expr_literal("42").into(),
        )
        .syntax()
        .clone_for_update();

        let node2 = make::item_const(
            [],
            None,
            make::name("BAR"),
            make::ty("i32"),
            make::expr_literal("43").into(),
        )
        .syntax()
        .clone_for_update();

        let id1 = map1.alloc(&node1);
        let id2_old = map2.alloc(&node2);

        // Merge map2 into map1
        let remapping = map1.merge(map2);

        // node1 should still have its original ID
        assert_eq!(map1.get_id(&node1), Some(id1));

        // node2 should have a new ID in map1
        let id2_new = map1.get_id(&node2).expect("node2 should be in map1 after merge");

        // The remapping should map old ID to new ID
        assert_eq!(remapping.get(&id2_old), Some(&id2_new));

        // Should be able to retrieve both nodes
        assert!(map1.get_node(id1).is_some());
        assert!(map1.get_node(id2_new).is_some());
    }

    #[test]
    fn test_merge_with_duplicate_positions() {
        let mut map1 = SyntaxIdMap::default();
        let mut map2 = SyntaxIdMap::default();

        // Create the same node in both maps
        let node = make::item_const(
            [],
            None,
            make::name("FOO"),
            make::ty("i32"),
            make::expr_literal("42").into(),
        )
        .syntax()
        .clone_for_update();

        let id1 = map1.alloc(&node);

        // Clone the node and allocate in map2 - different arena index
        let cloned_node = node.clone();
        let id2_old = map2.alloc(&cloned_node);

        // Merge map2 into map1
        let remapping = map1.merge(map2);

        // Since the node is at the same position, map1.alloc should return the existing id1
        assert_eq!(remapping.get(&id2_old), Some(&id1));

        // The node should still map to id1 in map1
        assert_eq!(map1.get_id(&node), Some(id1));
        assert_eq!(map1.get_id(&cloned_node), Some(id1));
    }

    #[test]
    fn test_round_trip() {
        let mut map = SyntaxIdMap::default();

        let node = make::item_const(
            [],
            None,
            make::name("FOO"),
            make::ty("i32"),
            make::expr_literal("42").into(),
        )
        .syntax()
        .clone_for_update();

        let id = map.alloc(&node);
        let retrieved_node = map.get_node(id).unwrap();

        assert_eq!(retrieved_node, &node);
        assert_eq!(map.get_id(retrieved_node), Some(id));
    }
}
