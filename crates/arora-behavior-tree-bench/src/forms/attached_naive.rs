use super::{build, Form, Nodes};
use crate::model::Workload;

pub struct AttachedNaive;
impl Form for AttachedNaive {
    fn name(&self) -> &'static str {
        "attached-naive"
    }
    fn build(&self, workload: &Workload) -> Nodes {
        build::build(workload, false)
    }
    fn fixture_counts(&self) -> [(u64, u64); 7] {
        [
            (3400, 5000),
            (1600, 1920),
            (1600, 1920),
            (1600, 1920),
            (1600, 1920),
            (2880, 3840),
            (2880, 4200),
        ]
    }
}
