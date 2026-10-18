//! Engine-local, behavior-tree-free test guest. The Status-returning version
//! lives in arora-sdk as test-rust-wasm-with-nodes.

use arora_types::AroraType;

/// What `reading` returns: an angle, its cosine and its quadrant. A record:
/// a header names it by id alone, so a device describes the functions taking
/// or returning it only from the module's record.
#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "8679542f-73d4-474e-a037-324f02a0cae7", version = "1.1.0")]
pub struct Reading {
    #[arora(id = "539dca7b-1122-45a5-ba3d-424da221c754")]
    pub angle: f32,
    #[arora(id = "184c40cc-2c53-4465-9570-edea6828c907")]
    pub cosine: f32,
    #[arora(id = "a9abfe94-c3be-441d-93ad-a01185b27523")]
    pub quadrant: Quadrant,
}

/// The quadrant an angle falls in.
#[derive(Debug, Clone, Copy, PartialEq, AroraType)]
#[arora(id = "424f4a46-f2f8-4a04-905c-b4f2dfb06322")]
pub enum Quadrant {
    #[arora(id = "d0878660-95b1-4742-b4d4-01b6721c7f0f")]
    First,
    #[arora(id = "b2973164-8913-4cdf-8d77-f8b9d0a0202e")]
    Second,
    #[arora(id = "3dcbdddc-33ff-4e5c-9245-70f0966e2c6e")]
    Third,
    #[arora(id = "5617b081-3add-4eb6-9789-f0f70fb20f16")]
    Fourth,
}

impl Reading {
    /// The reading of `angle`, in radians.
    pub fn of(angle: f32) -> Reading {
        let turn = angle.rem_euclid(std::f32::consts::TAU);
        let quadrant = match (turn / std::f32::consts::FRAC_PI_2) as u32 {
            0 => Quadrant::First,
            1 => Quadrant::Second,
            2 => Quadrant::Third,
            _ => Quadrant::Fourth,
        };
        Reading {
            angle,
            cosine: angle.cos(),
            quadrant,
        }
    }
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
    use super::Reading;

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

    /// A frame at `revision`: returns `length` values — value `i` is the time
    /// in seconds plus `i`, so it changes every step — and writes `revision`,
    /// the revision of the table the values are positioned by, into
    /// `positioned_by`. Both outputs need no value bound beforehand.
    #[export(id = "5b44c60c-0e52-432f-9a4c-9e14ff7edb12")]
    pub fn frame(
        #[param(id = "b1cce511-53dd-4754-9a15-71e0bafad3d5")] length: u32,
        #[param(id = "a54ac423-d9f6-455c-9b3f-f8e802ef2316")] revision: u64,
        #[param(id = "e5a9f7c1-3b8d-4e26-9f0a-6c1d2b3e4f50")] time_ns: u64,
        #[param(id = "8bc68040-2226-4e12-8f0c-0c947bcad035")] positioned_by: &mut Option<u64>,
    ) -> Vec<f64> {
        *positioned_by = Some(revision);
        let seconds = time_ns as f64 / 1e9;
        (0..length).map(|i| seconds + f64::from(i)).collect()
    }

    /// The reading of `angle`: a structure, naming an enumeration.
    #[export(id = "df544fe1-3832-47d8-94e9-233c41867480")]
    pub fn reading(#[param(id = "798768ab-0dc4-48c4-aef5-ba91ebe9b5ef")] angle: f32) -> Reading {
        Reading::of(angle)
    }

    /// The readings of `count` angles a quarter turn apart, from zero: an
    /// array of structures.
    #[export(id = "bc460484-914b-4be5-b291-bae39b9342a8")]
    pub fn readings(
        #[param(id = "d565ec30-53bc-4a9b-9951-78855a06dcf2")] count: u32,
    ) -> Vec<Reading> {
        (0..count)
            .map(|i| Reading::of(i as f32 * std::f32::consts::FRAC_PI_2))
            .collect()
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
        let mut positioned_by = None;
        assert_eq!(
            frame(2, 7, 1_500_000_000, &mut positioned_by),
            vec![1.5, 2.5]
        );
        assert_eq!(positioned_by, Some(7));
        assert_eq!(reading(0.0).quadrant, super::Quadrant::First);
        let quadrants: Vec<_> = readings(4).into_iter().map(|r| r.quadrant).collect();
        assert_eq!(
            quadrants,
            vec![
                super::Quadrant::First,
                super::Quadrant::Second,
                super::Quadrant::Third,
                super::Quadrant::Fourth
            ]
        );
    }
}
