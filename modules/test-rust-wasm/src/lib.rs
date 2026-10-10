//! Engine-local, behavior-tree-free test guest. The Status-returning version
//! lives in arora-sdk as test-rust-wasm-with-nodes.

/// A frame of values and the revision of the table they are positioned by:
/// what a module stepped each tick answers, for a behavior to check the
/// revision and write the values under a key table.
#[derive(Debug, Clone, PartialEq, arora_types::AroraType)]
#[arora(id = "d4c6574f-19ae-40b1-a377-2a77780a812e")]
pub struct Frame {
    /// The revision of the table `values` is positioned by.
    #[arora(id = "9340674c-b0b0-4ffc-96ae-f207b39dc09c")]
    pub revision: u64,
    /// One value per position.
    #[arora(id = "b629962e-57c6-4fdf-bf8c-a5b0065bc1ca")]
    pub values: Vec<f64>,
}

#[arora_module::module(
    id = "665d6ec9-3fc9-4cfa-9100-5c8964e95aec",
    name = "test-rust-wasm",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "Test WASM module written in Rust",
    executable_mime = "application/wasm"
)]
pub mod test_rust_wasm {
    use super::Frame;

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

    /// The length of the window from `start_ns` to `end_ns`, or `None` when it
    /// is open-ended (no `end_ns`).
    #[export(id = "66878cab-6396-480f-86f2-559dd534215e")]
    pub fn window(
        #[param(id = "0f4a5b8a-df7d-4ff9-98fc-123e53f7783a")] start_ns: u64,
        #[param(id = "3ccec224-b34c-415f-96d6-25a85e412ef3")] end_ns: Option<u64>,
    ) -> Option<u64> {
        end_ns.map(|end_ns| end_ns.saturating_sub(start_ns))
    }

    /// A frame of `length` values at `revision`: value `i` is the time in
    /// seconds plus `i`, so it changes every step.
    #[export(id = "5b44c60c-0e52-432f-9a4c-9e14ff7edb12")]
    pub fn frame(
        #[param(id = "b1cce511-53dd-4754-9a15-71e0bafad3d5")] length: u32,
        #[param(id = "a54ac423-d9f6-455c-9b3f-f8e802ef2316")] revision: u64,
        #[param(id = "e5a9f7c1-3b8d-4e26-9f0a-6c1d2b3e4f50")] time_ns: u64,
    ) -> Frame {
        let seconds = time_ns as f64 / 1e9;
        Frame {
            revision,
            values: (0..length).map(|i| seconds + f64::from(i)).collect(),
        }
    }

    /// A greeting, naming `name` when there is one.
    #[export(id = "c82c6987-656f-4fb1-9191-ec7a5cd6832b")]
    pub fn greet(
        #[param(id = "04cd4cb8-b7ab-40b5-84ae-0174fead4bba")] name: Option<String>,
    ) -> String {
        match name {
            Some(name) => format!("hello, {name}"),
            None => "hello".to_string(),
        }
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
        assert_eq!(window(10, Some(40)), Some(30));
        assert_eq!(window(10, None), None);
        assert_eq!(greet(Some("Ada".to_string())), "hello, Ada");
        assert_eq!(greet(None), "hello");
        assert_eq!(
            frame(2, 7, 1_500_000_000),
            crate::Frame {
                revision: 7,
                values: vec![1.5, 2.5],
            }
        );
    }
}
