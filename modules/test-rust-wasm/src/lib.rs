// Engine-local, behavior-tree-free copy. The Status-returning version lives in
// arora-sdk as test-rust-wasm-with-nodes.
#[allow(clippy::all, dead_code, unused)]
mod arora_generated;

fn ping() {}
fn succeed() -> bool {
    true
}

fn cos(angle: Option<f32>) -> f32 {
    angle.unwrap().cos()
}

fn add(a: Option<f32>, b: Option<f32>) -> f32 {
    a.unwrap() + b.unwrap()
}

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
