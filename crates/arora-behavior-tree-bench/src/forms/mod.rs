//! A form owns construction and lowering; harnesses use only this registry.
mod attached_naive;
mod detached;

use crate::{model::Workload, scale::Scenario};
use arora_behavior::FunctionIndex;
use arora_behavior_tree::{
    error::BehaviorTreeError, load_behavior_tree_nodes_with, lower_behavior_tree, schema::Node,
    variable::VariableResolver, LoweredTree,
};
use arora_types::call::CallBridge;
use std::{collections::HashMap, rc::Rc};
use uuid::Uuid;

pub type Nodes = (Vec<Node>, HashMap<Uuid, String>);

/// Named fixtures make each workload's total explicit in every form.
#[derive(Clone, Copy)]
pub enum Fixture {
    Synthetic,
    Conversation,
    Scale(Scenario),
}
impl Fixture {
    pub fn workload(self) -> Workload {
        match self {
            Self::Synthetic => crate::synthetic::workload(),
            Self::Conversation => crate::conversation::workload(),
            Self::Scale(s) => s.workload(10, 16, 144),
        }
    }
}

pub trait Form: Sync {
    fn name(&self) -> &'static str;
    fn build(&self, workload: &Workload) -> Nodes;

    /// A form may place nodes before loading, choose lowering options, and set
    /// flags on the lowered tree here. All harnesses call this same hook.
    fn lower(
        &self,
        nodes: Nodes,
        resolver: &Rc<VariableResolver<'static>>,
        functions: &FunctionIndex,
        bridge: &mut dyn CallBridge,
    ) -> Result<LoweredTree, BehaviorTreeError> {
        let tree = load_behavior_tree_nodes_with(nodes.0, &**resolver, &nodes.1)?
            .with_key_resolver(resolver.clone());
        lower_behavior_tree(tree, functions, bridge, false)
    }

    /// Total (module calls, dispatches), including warm-up, for a named fixture.
    /// A new form declares its count contract beside its implementation.
    fn fixture_counts(&self, fixture: Fixture) -> (u64, u64);
}

/// The reference is first. A form is one module and one entry here.
pub const REFERENCE: &str = "detached";

#[cfg(not(feature = "bench-check"))]
pub static FORMS: &[&dyn Form] = &[&detached::Detached, &attached_naive::AttachedNaive];

pub fn selected(names: Option<&str>) -> Result<Vec<&'static dyn Form>, String> {
    match names {
        None => Ok(FORMS.to_vec()),
        Some(names) => names
            .split(',')
            .map(|name| {
                FORMS
                    .iter()
                    .copied()
                    .find(|f| f.name() == name)
                    .ok_or_else(|| format!("unknown form: {name}"))
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|forms| {
                let mut seen = std::collections::HashSet::new();
                if forms.is_empty() || forms.iter().any(|f| !seen.insert(f.name())) {
                    Err("empty or duplicate form selection".into())
                } else {
                    Ok(forms)
                }
            }),
    }
}

#[cfg(feature = "bench-check")]
pub static FORMS: &[&dyn Form] = &[
    &detached::Detached,
    &attached_naive::AttachedNaive,
    &crate::checks::Wrong,
];
