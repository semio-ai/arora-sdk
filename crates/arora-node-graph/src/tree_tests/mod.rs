//! The node graph's tests, run on the behavior tree: each file here is the
//! counterpart of the reference evaluator's test file of the same name in
//! [`crate::eval`], with the graph evaluated by a [`TreeRuntime`] where the
//! reference tests use a [`GraphRuntime`](crate::GraphRuntime).
//!
//! A test about the reference evaluator's own machinery (its plan cache, its
//! evaluation for writes) holds the same of the tree, and is named for what
//! it holds there:
//!
//! | Reference test | On the tree |
//! |---|---|
//! | `plan_cache_reuses_layouts_when_spec_version_matches` | `a_spec_evaluated_again_keeps_its_tree` |
//! | `plan_cache_rebuilds_when_spec_version_changes` | `a_spec_of_another_version_builds_its_tree_again` |
//! | `an_evaluation_for_writes_writes_what_a_full_one_writes` | `an_evaluation_of_what_the_outputs_read_writes_what_a_full_one_writes` |
//! | `a_selection_from_a_constant_is_planned_and_follows_its_value` | `a_selection_from_a_constant_reads_it_once_and_follows_its_value` |
//!
//! [`structure_tests`] holds the counterparts of the topology, plan and
//! selection tests. [`equivalence`] runs every fixture on both engines.
//!
//! [`TreeRuntime`]: crate::tree_runtime::TreeRuntime
mod blend_tests;
mod equivalence;
mod external_function_tests;
mod required_inputs_tests;
mod spring_tests;
mod structure_tests;
mod tests;
mod zero_dt_tests;
