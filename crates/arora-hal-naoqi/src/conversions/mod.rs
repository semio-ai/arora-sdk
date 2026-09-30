//! Pure conversions between NAOqi names and values and Arora keys and values.

pub mod keys;

mod joints;
pub(crate) use joints::JointTable;

mod values;
pub(crate) use values::{arora_f64, qi_f64, qi_list};
