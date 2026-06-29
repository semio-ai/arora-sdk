use crate::{
    error::BehaviorTreeError,
    schema::{Expression, Node, NodeParameterId},
    setup_node_parameter_variable,
    variable::{VariableCell, VariableResolver},
    BehaviorTree,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use uuid::Uuid;

// Helpers to make trees
//================================================================
pub struct TreeNode {
    pub function: Uuid,
    pub children: Option<Vec<TreeNode>>,
    pub parameters: HashMap<Uuid, Expression>,
}

/// Represents a tree of node with direct relations to children,
/// instead of using UUID references.
#[allow(unused)]
impl TreeNode {
    /// Helper to construct an action node.
    pub fn action_node(function: Uuid) -> Self {
        Self {
            function,
            children: None,
            parameters: HashMap::new(),
        }
    }

    /// Helper to construct a control node.
    pub fn control_node(function: Uuid, children: Vec<TreeNode>) -> Self {
        Self {
            function,
            children: Some(children),
            parameters: HashMap::new(),
        }
    }

    /// Moves the data to this node into components of a behavior tree, recursively.
    ///
    /// `resolver` + `names` bind each first-referenced `{var}` to a data-store
    /// slot when its name is known (see [`setup_node_parameter_variable`]); pass
    /// `&|_| None` and an empty map to keep every cell tree-local.
    pub fn collect(
        self,
        mut node_index: &mut HashMap<Uuid, Rc<Node>>,
        mut variables: &mut HashMap<Uuid, VariableCell>,
        mut node_parameters_variables: &mut HashMap<NodeParameterId, VariableCell>,
        resolver: &VariableResolver,
        names: &HashMap<Uuid, String>,
    ) -> Result<Rc<Node>, BehaviorTreeError> {
        let node_id = Uuid::new_v4();
        let children: Option<Vec<Uuid>> = if let Some(children) = self.children {
            let mut ids = Vec::with_capacity(children.len());
            for child in children {
                let child_node = child.collect(
                    node_index,
                    variables,
                    node_parameters_variables,
                    resolver,
                    names,
                )?;
                let child_node_id = child_node.id;
                // This could only happen with an UUID collision, i.e. never.
                assert_eq!(node_index.insert(child_node.id, child_node), None);
                ids.push(child_node_id);
            }
            Some(ids)
        } else {
            None
        };
        let mut arguments = HashMap::new();
        for (param_id, expression) in self.parameters {
            let node_parameter = NodeParameterId {
                node: node_id.to_owned(),
                parameter: param_id.to_owned(),
            };
            setup_node_parameter_variable(
                &node_parameter,
                &expression,
                variables,
                node_parameters_variables,
                resolver,
                names,
            )?;
            arguments.insert(param_id, expression);
        }
        Ok(Rc::new(Node {
            id: node_id,
            function: self.function,
            arguments,
            children,
        }))
    }

    /// Transforms the tree of nodes into a runnable behavior tree, binding each
    /// `{var}` to a data-store slot when `resolver` resolves its name (the Direct
    /// convention — variable name == store key). `names` maps each variable id to
    /// the name the resolver expects; ids absent from it (or that the resolver
    /// declines) stay tree-local.
    pub fn into_behavior_tree(
        self,
        resolver: &VariableResolver,
        names: &HashMap<Uuid, String>,
    ) -> Result<BehaviorTree, BehaviorTreeError> {
        let mut node_index = HashMap::new();
        let mut variables = HashMap::new();
        let mut node_arg_variables = HashMap::new();
        let root = self.collect(
            &mut node_index,
            &mut variables,
            &mut node_arg_variables,
            resolver,
            names,
        )?;
        Ok(BehaviorTree {
            root,
            node_index,
            variables: Rc::new(RefCell::new(variables)),
            node_arg_variables: Rc::new(node_arg_variables),
        })
    }
}

/// Transforms the tree of nodes into a behavior tree that can be run.
///
/// All cells stay tree-local — to bind `{var}`s to a data store, call
/// [`TreeNode::into_behavior_tree`] with a resolver instead.
impl TryInto<BehaviorTree> for TreeNode {
    type Error = BehaviorTreeError;
    fn try_into(self) -> Result<BehaviorTree, Self::Error> {
        self.into_behavior_tree(&|_| None, &HashMap::new())
    }
}
