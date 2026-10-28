use super::{build, Form, Nodes};
use crate::model::Workload;

pub struct Detached;
impl Form for Detached {
    fn name(&self) -> &'static str {
        super::REFERENCE
    }
    fn build(&self, workload: &Workload) -> Nodes {
        build::build(workload, true)
    }
    fn fixture_counts(&self) -> [(u64, u64); 7] {
        [
            (1846, 1800),
            (10, 160),
            (525, 160),
            (1040, 160),
            (487, 160),
            (640, 1120),
            (640, 1480),
        ]
    }
}
