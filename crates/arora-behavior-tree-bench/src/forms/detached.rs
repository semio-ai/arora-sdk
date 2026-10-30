use super::{Fixture, Form, Nodes};
use crate::model::Workload;

pub struct Detached;
impl Form for Detached {
    fn name(&self) -> &'static str {
        super::REFERENCE
    }
    fn build(&self, workload: &Workload) -> Nodes {
        workload.nodes.clone()
    }
    fn fixture_counts(&self, fixture: Fixture) -> (u64, u64) {
        use crate::scale::Scenario;
        match fixture {
            Fixture::Synthetic => (1934, 2000),
            Fixture::Conversation => (364_889, 309_231),
            Fixture::Scale(Scenario::Settled) => (10, 160),
            Fixture::Scale(Scenario::OneInput) => (525, 160),
            Fixture::Scale(Scenario::AllInputs) => (1040, 160),
            Fixture::Scale(Scenario::Clock) => (1144, 160),
            Fixture::Scale(Scenario::Branches) => (640, 1120),
            Fixture::Scale(Scenario::BranchesSwitching) => (640, 1480),
        }
    }
}
