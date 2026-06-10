// Let us pretend this is an auto-generated header.
//====================================================================================
// Provides the interface of the module, and forwards calls to specialized functions.
// Generated code: allow clippy/dead_code over the whole generated subtree.
#[allow(clippy::all, dead_code)]
mod arora_generated;
// Provides symbols imported from other modules.
use arora_generated::behavior_tree::status::Status;
//====================================================================================
// Put the implementation below.

fn ping() {}
fn succeed() -> Status {
    Status::Success
}

fn cos(angle: Option<f32>) -> f32 {
    angle.unwrap().cos()
}

fn add(a: Option<f32>, b: Option<f32>) -> f32 {
    a.unwrap() + b.unwrap()
}

// Tests
//====================================================================================
#[cfg(test)]
pub mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    pub fn test_ping() -> Result<()> {
        ping();
        Ok(())
    }
}
