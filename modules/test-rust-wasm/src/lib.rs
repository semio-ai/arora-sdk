//! Engine-local, behavior-tree-free test guest. The Status-returning version
//! lives in arora-sdk as test-rust-wasm-with-nodes.

#[arora_module::module(
    id = "665d6ec9-3fc9-4cfa-9100-5c8964e95aec",
    name = "test-rust-wasm",
    version = "0.1.0",
    author = "Semio",
    license = "Proprietary",
    description = "Test WASM module written in Rust",
    executable_mime = "application/wasm"
)]
pub mod test_rust_wasm {
    #[export(id = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6")]
    pub fn ping() {}

    #[export(id = "00cd31a8-2cf4-48e6-a957-69a55de90424")]
    pub fn succeed() -> bool {
        true
    }

    #[export(id = "c13757cb-2311-4c93-abcc-cb12d6cbb859")]
    pub fn cos(#[param(id = "6c2a157c-4235-47b0-bff3-1eeef3e5747d")] angle: f32) -> f32 {
        angle.cos()
    }

    #[export(id = "e4b0a2f3-6c7d-4e8f-9a0b-1c2d3e4f5a6b")]
    pub fn add(
        #[param(id = "a1b2c3d4-e5f6-4a8b-9c0d-e1f2a3b4c5d6")] a: f32,
        #[param(id = "b2c3d4e5-f6a7-4b9c-8d1e-f2a3b4c5d6e7")] b: f32,
    ) -> f32 {
        a + b
    }

    /// A string of `length` bytes, so a host can choose the size of the result.
    #[export(id = "431fb111-d750-4470-b1c8-94b0ad8c6e6b")]
    pub fn text(#[param(id = "64351cbc-d84b-4df3-bd52-fd23edd943cf")] length: u32) -> String {
        "x".repeat(length as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::test_rust_wasm::*;

    #[test]
    fn ping_answers() {
        ping();
        assert!(succeed());
        assert_eq!(add(2.0, 3.0), 5.0);
    }
}
