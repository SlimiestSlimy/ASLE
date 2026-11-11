//! A binary code tree: leaves carry a leaf index, left = 0, right = 1.
//! Built by huffman_tree (for decisions) or order_preserving_code_tree (for
//! the container); written, read and walked here, always depth first.

use crate::bit_io::{BitReader, BitWriter};

#[derive(Clone, Copy)]
pub enum CodeTreeNode {
    Leaf { leaf_index: usize },
    Internal { left_child_node_id: usize, right_child_node_id: usize },
}

pub struct BinaryCodeTree {
    pub nodes: Vec<CodeTreeNode>,
    pub root_node_id: usize,
}

impl BinaryCodeTree {
    /// (code bits, code length) for every leaf index below `leaf_count`.
    /// A tree that is a single leaf gives that leaf a 0-bit code.
    pub fn calculate_codes_by_leaf_index(&self, leaf_count: usize) -> Vec<(u64, u32)> {
        let mut codes_by_leaf_index = vec![(0u64, 0u32); leaf_count];
        let mut leaf_already_coded = vec![false; leaf_count];
        let mut nodes_to_visit = vec![(self.root_node_id, 0u64, 0u32)];
        while let Some((node_id, code_bits, code_length)) = nodes_to_visit.pop() {
            match self.nodes[node_id] {
                CodeTreeNode::Leaf { leaf_index } => {
                    if !leaf_already_coded[leaf_index] {
                        leaf_already_coded[leaf_index] = true;
                        codes_by_leaf_index[leaf_index] = (code_bits, code_length);
                    }
                }
                CodeTreeNode::Internal { left_child_node_id, right_child_node_id } => {
                    assert!(code_length < 64, "code longer than 64 bits");
                    nodes_to_visit.push((right_child_node_id, (code_bits << 1) | 1, code_length + 1));
                    nodes_to_visit.push((left_child_node_id, code_bits << 1, code_length + 1));
                }
            }
        }
        codes_by_leaf_index
    }

    /// Depth-first: bit 1 = internal node (then left, then right subtree),
    /// bit 0 = leaf (then whatever `write_leaf_payload` writes for it).
    pub fn write_depth_first(&self, bit_writer: &mut BitWriter, write_leaf_payload: &mut impl FnMut(&mut BitWriter, usize)) {
        self.write_node_depth_first(self.root_node_id, bit_writer, write_leaf_payload);
    }

    fn write_node_depth_first(
        &self,
        node_id: usize,
        bit_writer: &mut BitWriter,
        write_leaf_payload: &mut impl FnMut(&mut BitWriter, usize),
    ) {
        match self.nodes[node_id] {
            CodeTreeNode::Leaf { leaf_index } => {
                bit_writer.write_bit(0);
                write_leaf_payload(bit_writer, leaf_index);
            }
            CodeTreeNode::Internal { left_child_node_id, right_child_node_id } => {
                bit_writer.write_bit(1);
                self.write_node_depth_first(left_child_node_id, bit_writer, write_leaf_payload);
                self.write_node_depth_first(right_child_node_id, bit_writer, write_leaf_payload);
            }
        }
    }

    /// Inverse of `write_depth_first`; `read_leaf_payload` returns the leaf index.
    pub fn read_depth_first(
        bit_reader: &mut BitReader,
        read_leaf_payload: &mut impl FnMut(&mut BitReader) -> usize,
    ) -> BinaryCodeTree {
        let mut nodes = Vec::new();
        let root_node_id = Self::read_node_depth_first(bit_reader, read_leaf_payload, &mut nodes);
        BinaryCodeTree { nodes, root_node_id }
    }

    fn read_node_depth_first(
        bit_reader: &mut BitReader,
        read_leaf_payload: &mut impl FnMut(&mut BitReader) -> usize,
        nodes: &mut Vec<CodeTreeNode>,
    ) -> usize {
        if bit_reader.read_bit() == 0 {
            let leaf_index = read_leaf_payload(bit_reader);
            nodes.push(CodeTreeNode::Leaf { leaf_index });
        } else {
            let left_child_node_id = Self::read_node_depth_first(bit_reader, read_leaf_payload, nodes);
            let right_child_node_id = Self::read_node_depth_first(bit_reader, read_leaf_payload, nodes);
            nodes.push(CodeTreeNode::Internal { left_child_node_id, right_child_node_id });
        }
        nodes.len() - 1
    }

    /// Walks from the root following bits until a leaf; returns its leaf index.
    pub fn decode_next_leaf_index(&self, bit_reader: &mut BitReader) -> usize {
        let mut node_id = self.root_node_id;
        loop {
            match self.nodes[node_id] {
                CodeTreeNode::Leaf { leaf_index } => return leaf_index,
                CodeTreeNode::Internal { left_child_node_id, right_child_node_id } => {
                    node_id = if bit_reader.read_bit() == 0 { left_child_node_id } else { right_child_node_id };
                }
            }
        }
    }

}
