use super::{Form, Nodes};
use crate::model::Workload;

pub struct Detached;
impl Form for Detached {
    fn name(&self) -> &'static str {
        super::REFERENCE
    }
    fn build(&self, workload: &Workload) -> Nodes {
        workload.nodes.clone()
    }
    fn fixture_counts(&self) -> [(u64, u64); 7] {
        [
            (1934, 2000),
            (10, 160),
            (525, 160),
            (1040, 160),
            (1144, 160),
            (640, 1120),
            (640, 1480),
        ]
    }
}
