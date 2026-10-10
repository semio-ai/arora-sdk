//! The HAL as a module: the well-known ids under which the runtime exposes
//! its one [`Hal`](crate::Hal) on the engine.
//!
//! The runtime assembles a host module under [`ID`] whose functions answer
//! from the device's HAL, through [`Hal::assets`](crate::Hal::assets):
//!
//! - [`MODELS`] → [`HalAssets::models`](crate::HalAssets::models): no
//!   argument, returns the model of each component that has one, an array of
//!   [`ComponentModel`](crate::ComponentModel);
//! - [`MODEL_GLB`] → one component's model as `Option<bytes>`, taking the
//!   component's name ([`MODEL_GLB_COMPONENT`]): `None` when the component has
//!   no model or its model is not servable;
//! - [`MODEL_GLBS`] → every servable model, an array of
//!   [`ComponentGlb`](crate::ComponentGlb).
//!
//! A device's models are fixed for its life, so a client asks once. The
//! module describes its functions, so a remote finds them in the device's
//! method list, and a call reaches them through the engine's normal dispatch,
//! like any module function. The design is `docs/proposal-hal-components.md`.

use arora_types::Uuid;

/// Module id under which the runtime registers the HAL module. Self-identifying
/// like the interpreter module's: the ASCII bytes of "arorahal" lead the UUID,
/// a small offset tails it.
pub const ID: Uuid = Uuid::from_u128(0x61726f72_6168_616c_0000_000000000001);

/// Function id of **models**: the model of each component that has one.
pub const MODELS: Uuid = Uuid::from_u128(0x61726f72_6168_616c_0000_000000000002);

/// Function id of **model_glb**: one component's model, when servable.
pub const MODEL_GLB: Uuid = Uuid::from_u128(0x61726f72_6168_616c_0000_000000000003);

/// Parameter id of [`MODEL_GLB`]'s one argument: the component's name, a
/// `String`.
pub const MODEL_GLB_COMPONENT: Uuid = Uuid::from_u128(0x61726f72_6168_616c_0000_000000000004);

/// Function id of **model_glbs**: every servable model.
pub const MODEL_GLBS: Uuid = Uuid::from_u128(0x61726f72_6168_616c_0000_000000000005);
