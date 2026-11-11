//! The optimal binary code tree whose leaves keep a given order (an optimal
//! alphabetic code), by the Garsia–Wachs algorithm with linear memory.
//!
//! The container lists symbols in depth-first prefix-tree order, and the code
//! tree's leaves follow that same order, so the decoder needs only the code
//! tree's shape to know which code belongs to which symbol.
//!
//! Phase 1 combines weights to find every leaf's optimal depth; phase 2
//! builds the tree with those depths, leaves left to right in the given order.

use crate::code_tree::{BinaryCodeTree, CodeTreeNode};

/// Leaf i of the result is the i-th weight. All weights must be > 0.
pub fn build_order_preserving_optimal_code_tree(weights_in_order: &[u64]) -> BinaryCodeTree {
    assert!(!weights_in_order.is_empty(), "a code tree needs at least one leaf");
    let leaf_depths = find_optimal_leaf_depths(weights_in_order);
    build_tree_with_leaf_depths_in_order(&leaf_depths)
}

/// Garsia–Wachs phase 1: repeatedly combine the leftmost pair (a, b) whose
/// next weight c satisfies a <= c (a missing c counts as infinite), then move
/// the combined weight left past every smaller weight before it. The depths
/// of the leaves in the resulting tree are optimal for an order-keeping tree.
fn find_optimal_leaf_depths(weights_in_order: &[u64]) -> Vec<u32> {
    let leaf_count = weights_in_order.len();
    // Combination tree, used only for depths: leaves are nodes 0..leaf_count.
    let mut parent_node_id: Vec<usize> = vec![usize::MAX; leaf_count];
    // The working sequence: (weight, combination node id).
    let mut working_sequence: Vec<(u64, usize)> =
        weights_in_order.iter().enumerate().map(|(leaf_index, &weight)| (weight, leaf_index)).collect();
    while working_sequence.len() > 1 {
        let length = working_sequence.len();
        let pair_start = (0..length - 1)
            .find(|&index| index + 2 >= length || working_sequence[index].0 <= working_sequence[index + 2].0)
            .unwrap();
        let (left_weight, left_node_id) = working_sequence[pair_start];
        let (right_weight, right_node_id) = working_sequence[pair_start + 1];
        let combined_node_id = parent_node_id.len();
        parent_node_id.push(usize::MAX);
        parent_node_id[left_node_id] = combined_node_id;
        parent_node_id[right_node_id] = combined_node_id;
        let combined_weight = left_weight + right_weight;
        working_sequence.drain(pair_start..pair_start + 2);
        let mut insert_position = pair_start;
        while insert_position > 0 && working_sequence[insert_position - 1].0 < combined_weight {
            insert_position -= 1;
        }
        working_sequence.insert(insert_position, (combined_weight, combined_node_id));
    }
    // Parents are created after their children, so walking ids downward
    // always meets a parent before its children.
    let mut node_depth = vec![0u32; parent_node_id.len()];
    for node_id in (0..parent_node_id.len()).rev() {
        if parent_node_id[node_id] != usize::MAX {
            node_depth[node_id] = node_depth[parent_node_id[node_id]] + 1;
        }
    }
    node_depth[..leaf_count].to_vec()
}

/// Builds a tree whose leaves, left to right, are 0..len with the given depths:
/// push each leaf; while the top two entries share a depth, join them into
/// their parent one level up.
fn build_tree_with_leaf_depths_in_order(leaf_depths: &[u32]) -> BinaryCodeTree {
    let mut nodes: Vec<CodeTreeNode> = Vec::new();
    let mut pending: Vec<(usize, u32)> = Vec::new(); // (node id, depth)
    for (leaf_index, &depth) in leaf_depths.iter().enumerate() {
        nodes.push(CodeTreeNode::Leaf { leaf_index });
        pending.push((nodes.len() - 1, depth));
        while pending.len() >= 2 && pending[pending.len() - 1].1 == pending[pending.len() - 2].1 {
            let (right_child_node_id, depth) = pending.pop().unwrap();
            let (left_child_node_id, _) = pending.pop().unwrap();
            nodes.push(CodeTreeNode::Internal { left_child_node_id, right_child_node_id });
            pending.push((nodes.len() - 1, depth - 1));
        }
    }
    assert!(pending.len() == 1 && pending[0].1 == 0, "leaf depths do not form a complete tree");
    BinaryCodeTree { root_node_id: pending[0].0, nodes }
}
