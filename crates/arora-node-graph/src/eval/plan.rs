//! The port layouts of a node: its inputs and outputs, in slot order.

use crate::ports::{ports, Ports};
use crate::types::{InputConnection, NodeSpec, NodeType};
use hashbrown::{HashMap, HashSet};

use super::variadic::{compare_variadic_keys, parse_variadic_key};

#[derive(Clone, Copy, Debug, Default)]
pub struct VariadicRange {
    pub start: usize,
    pub len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PortLayout {
    pub slots: Vec<String>,
    name_to_slot: HashMap<String, usize>,
    variadics: HashMap<String, VariadicRange>,
}

impl PortLayout {
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.name_to_slot.get(name).copied()
    }

    pub fn slot_name(&self, slot: usize) -> Option<&str> {
        self.slots.get(slot).map(String::as_str)
    }

    pub fn variadic_range(&self, group: &str) -> Option<VariadicRange> {
        self.variadics.get(group).copied()
    }

    pub fn variadic_groups(&self) -> &HashMap<String, VariadicRange> {
        &self.variadics
    }

    fn insert_slot(&mut self, name: String) {
        if !self.name_to_slot.contains_key(&name) {
            let slot = self.slots.len();
            self.slots.push(name.clone());
            self.name_to_slot.insert(name, slot);
        }
    }
}

fn build_input_layout(ports: Ports, connections: &HashMap<String, InputConnection>) -> PortLayout {
    let mut layout = PortLayout::default();

    // Fixed inputs come first to keep indices stable.
    for port in ports.inputs {
        layout.insert_slot(port.to_string());
    }

    let variadic_id = ports.variadic_inputs.map(|v| v.id);
    let mut variadic_keys: Vec<(String, Option<usize>)> = Vec::new();

    for key in connections.keys() {
        if let Some(var_name) = variadic_id {
            let (prefix, _) = parse_variadic_key(key);
            if prefix == var_name {
                let (_, idx) = parse_variadic_key(key);
                variadic_keys.push((key.clone(), idx));
                continue;
            }
        }

        layout.insert_slot(key.clone());
    }

    if let Some(var_name) = variadic_id {
        variadic_keys.sort_by(|(a, _), (b, _)| compare_variadic_keys(a, b));
        let start = layout.slots.len();
        for (key, _) in &variadic_keys {
            layout.insert_slot(key.clone());
        }
        layout.variadics.insert(
            var_name.to_string(),
            VariadicRange {
                start,
                len: variadic_keys.len(),
            },
        );
    }

    layout
}

fn build_output_layout(node: &NodeSpec, ports: Ports, referenced: &HashSet<String>) -> PortLayout {
    let mut layout = PortLayout::default();

    for port in ports.outputs {
        layout.insert_slot(port.to_string());
    }

    // Respect any explicit output_shapes hints.
    for key in node.output_shapes.keys() {
        layout.insert_slot(key.clone());
    }

    {
        if let Some(var_out) = &ports.variadic_outputs {
            let var_id = var_out.id;

            // Detect canonical {id}_{N} references via parse_variadic_key.
            // The frontend uses 0-indexed naming: elements_0, elements_1, ...
            let mut has_canonical = false;
            let mut max_canonical_idx: Option<usize> = None;
            for name in referenced.iter() {
                let (prefix, idx_opt) = parse_variadic_key(name);
                if prefix == var_id {
                    if let Some(idx) = idx_opt {
                        has_canonical = true;
                        max_canonical_idx =
                            Some(max_canonical_idx.map_or(idx, |prev: usize| prev.max(idx)));
                    }
                }
            }

            // Detect legacy concatenated {stem}{N} references (e.g. "part1" for id "parts").
            let stem = var_id.strip_suffix('s').unwrap_or(var_id);
            let mut max_legacy = 0usize;
            if stem != var_id {
                for name in referenced.iter() {
                    if let Some(tail) = name.strip_prefix(stem) {
                        if let Ok(idx) = tail.parse::<usize>() {
                            max_legacy = max_legacy.max(idx);
                        }
                    }
                }
            }

            // For canonical refs, count = max_index + 1 (0-indexed).
            // For legacy refs, count = max_index (1-indexed, so max IS the count).
            let canonical_count = max_canonical_idx.map(|m| m + 1).unwrap_or(0);
            let max_ref = std::cmp::max(canonical_count, max_legacy);

            // Node-type-specific param-based minimum count.
            let min_from_params = match node.kind {
                NodeType::Split => node.params.sizes.as_ref().map(|v| v.len()).unwrap_or(0),
                NodeType::ReadRecord | NodeType::TaskRun => node
                    .params
                    .record_keys
                    .as_ref()
                    .map(|v| v.len())
                    .unwrap_or(0),
                NodeType::FromVector => node.params.sizes.as_ref().map(|v| v.len()).unwrap_or(0),
                _ => 0,
            };

            let count = std::cmp::max(1, std::cmp::max(min_from_params, max_ref));
            let start = layout.slots.len();

            if var_out.keyed || has_canonical {
                // Frontend convention: 0-indexed {id}_{N} (elements_0, elements_1, ...).
                // Also used for keyed variadics (e.g. ReadRecord fields_0, fields_1).
                for i in 0..count {
                    layout.insert_slot(format!("{}_{}", var_id, i));
                }
            } else {
                // Legacy concatenated convention: {stem}{N} (1-indexed).
                for i in 0..count {
                    layout.insert_slot(format!("{}{}", stem, i + 1));
                }
            }

            let range = VariadicRange { start, len: count };
            layout.variadics.insert(var_id.to_string(), range);
            // Also register under the stem for backward eval-function compat.
            if stem != var_id {
                layout.variadics.insert(stem.to_string(), range);
            }
        }
    }

    // Add any referenced outputs we haven't seen yet to keep slot lookups stable.
    for name in referenced {
        layout.insert_slot(name.clone());
    }

    layout
}

/// The input layout a node of `kind` gets when the inputs `names` are wired:
/// its kind's fixed inputs first, then the other ports wired, then its
/// variadic group in key order.
pub(crate) fn input_layout<'a>(
    kind: &NodeType,
    names: impl IntoIterator<Item = &'a str>,
) -> PortLayout {
    let connections: HashMap<String, InputConnection> = names
        .into_iter()
        .map(|name| (name.to_string(), InputConnection::default()))
        .collect();
    build_input_layout(ports(kind), &connections)
}

/// The output layout of `node`, whose ports `referenced` are read by an edge:
/// its kind's outputs, its declared output shapes, its variadic outputs and
/// every referenced port.
pub(crate) fn output_layout(node: &NodeSpec, referenced: &HashSet<String>) -> PortLayout {
    build_output_layout(node, ports(&node.kind), referenced)
}

/// A port layout of exactly `names`, in order, with the variadic groups
/// `variadics` (each a group name and its range of slots).
pub(crate) fn layout_of(
    names: impl IntoIterator<Item = String>,
    variadics: impl IntoIterator<Item = (String, VariadicRange)>,
) -> PortLayout {
    let mut layout = PortLayout::default();
    for name in names {
        layout.insert_slot(name);
    }
    layout.variadics.extend(variadics);
    layout
}
