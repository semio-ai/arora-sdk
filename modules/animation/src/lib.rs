//! `arora-animation` as the Arora module `@arora/animation`: a
//! `wasm32-wasip2` component, which a device loads from its module directory
//! (`module_dir`, the `module-dir` feature), or host-linked, with a state per
//! registration.
//!
//! The functions are declared twice, with the same ids: as a contract
//! ([`AnimationContract`]), whose methods take the state they act on, so a
//! host that runs many devices in one process registers one state per device
//! (`HostModule::from_exports(animation::ids::MODULE, exports(AnimationModule::new()))`);
//! and as the module [`animation`], whose free functions act on the guest's
//! one state and give the guest its entry points and its header. A test
//! holds the two declarations equal.
mod convert;
#[cfg(feature = "module-dir")]
pub mod module_dir;
mod records;
mod state;
use arora_types::module::declared::AroraFunction;
pub use convert::ConversionError;
pub use records::*;
pub use state::AnimationModule;

/// The module's functions, on the state of one registration. See the crate's
/// readme for what each does.
#[arora_module::contract(name = "animation")]
pub trait AnimationContract {
    #[export(id = "76697a69-6a00-0000-0f00-000000000001")]
    fn load_animation(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f01-000000000001")] clip: AnimationClip,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000002")]
    fn create_player(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f02-000000000001")] name: Option<String>,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000003")]
    fn add_instance(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f03-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f03-000000000002")] anim: u32,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000004")]
    fn step(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f04-000000000001")] dt_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f04-000000000002")] time_ns: Option<u64>,
    ) -> Vec<TrackOutput>;
    #[export(id = "76697a69-6a00-0000-0f00-000000000005")]
    fn play(&mut self, #[param(id = "76697a69-6a00-0000-0f05-000000000001")] player: u32) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000006")]
    fn pause(&mut self, #[param(id = "76697a69-6a00-0000-0f06-000000000001")] player: u32) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000007")]
    fn stop(&mut self, #[param(id = "76697a69-6a00-0000-0f07-000000000001")] player: u32) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000008")]
    fn seek(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f08-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f08-000000000002")] time_ns: u64,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000009")]
    fn set_speed(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f09-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f09-000000000002")] speed: f32,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-00000000000a")]
    fn set_loop(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f0a-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f0a-000000000002")] mode: String,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-00000000000b")]
    fn set_weight(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f0b-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f0b-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f0b-000000000003")] weight: f32,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-00000000000c")]
    fn remove_instance(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f0c-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f0c-000000000002")] instance: u32,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-00000000000d")]
    fn player_states(&mut self) -> Vec<PlayerState>;
    #[export(id = "76697a69-6a00-0000-0f00-00000000000e")]
    fn bake(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f0e-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f0e-000000000002")] frame_rate: Option<f32>,
        #[param(id = "76697a69-6a00-0000-0f0e-000000000003")] start_ns: Option<u64>,
        #[param(id = "76697a69-6a00-0000-0f0e-000000000004")] end_ns: Option<u64>,
    ) -> Option<BakedAnimation>;
    #[export(id = "76697a69-6a00-0000-0f00-00000000000f")]
    fn bake_with_derivatives(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f0f-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f0f-000000000002")] frame_rate: Option<f32>,
        #[param(id = "76697a69-6a00-0000-0f0f-000000000003")] start_ns: Option<u64>,
        #[param(id = "76697a69-6a00-0000-0f0f-000000000004")] end_ns: Option<u64>,
    ) -> Option<BakedAnimation>;
    #[export(id = "76697a69-6a00-0000-0f00-000000000010")]
    fn unload_animation(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f10-000000000001")] anim: u32,
    ) -> bool;
    #[export(id = "76697a69-6a00-0000-0f00-000000000011")]
    fn remove_player(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f11-000000000001")] player: u32,
    ) -> bool;
    #[export(id = "76697a69-6a00-0000-0f00-000000000012")]
    fn add_instance_with_weight(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f12-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f12-000000000002")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f12-000000000003")] weight: f32,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000013")]
    fn reload_animation(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f13-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f13-000000000002")] clip: AnimationClip,
    ) -> bool;
    #[export(id = "76697a69-6a00-0000-0f00-000000000014")]
    fn set_window(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f14-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f14-000000000002")] start_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f14-000000000003")] end_ns: Option<u64>,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000015")]
    fn play_at(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f15-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f15-000000000002")] time_ns: u64,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000016")]
    fn set_start_offset(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f16-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f16-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f16-000000000003")] offset_ns: i64,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000017")]
    fn set_time_scale(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f17-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f17-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f17-000000000003")] time_scale: f32,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-000000000018")]
    fn step_values(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f18-000000000001")] dt_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f18-000000000002")] time_ns: Option<u64>,
    ) -> StepValues;
    #[export(id = "76697a69-6a00-0000-0f00-000000000019")]
    fn output_keys(&mut self) -> OutputKeys;
    #[export(id = "76697a69-6a00-0000-0f00-00000000001a")]
    fn load_clip(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f1a-000000000001")] clip: Clip,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-00000000001b")]
    fn reload_clip(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f1b-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f1b-000000000002")] clip: Clip,
    ) -> bool;
    #[export(id = "76697a69-6a00-0000-0f00-00000000001c")]
    fn set_rate(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f1c-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f1c-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f1c-000000000003")] rate: f64,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-00000000001d")]
    fn step_with_velocities(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f1d-000000000001")] dt_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f1d-000000000002")] time_ns: Option<u64>,
    ) -> StepSamples;
    #[export(id = "76697a69-6a00-0000-0f00-00000000001e")]
    fn set_length(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f1e-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f1e-000000000002")] length_ns: Option<u64>,
    ) -> u32;
    #[export(id = "76697a69-6a00-0000-0f00-00000000001f")]
    fn set_reversed(
        &mut self,
        #[param(id = "76697a69-6a00-0000-0f1f-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f1f-000000000002")] reversed: bool,
    ) -> u32;
}

/// Every function, acting on `state` alone: one registration's.
pub fn exports(state: AnimationModule) -> Vec<AroraFunction> {
    animation_contract::exports(state)
}

thread_local! {
    static GUEST: std::cell::UnsafeCell<AnimationModule> = std::cell::UnsafeCell::new(AnimationModule::new());
}

fn guest<T>(f: impl FnOnce(&mut AnimationModule) -> T) -> T {
    GUEST.with(|state| {
        // SAFETY: calls are synchronous on this thread, never reenter the
        // guest, and no reference to the state escapes the call.
        f(unsafe { &mut *state.get() })
    })
}

// Built for wasm32-wasip2, the guest is a component serving `animation`.
arora_module::artifact!(animation);

/// The module as the guest declares it: its header, and an entry point per
/// function, each acting on the guest's state.
#[arora_module::module(
    id = "76697a69-6a00-0000-0d00-000000000000",
    name = "animation",
    version = "3.0.0",
    scope = arora_types::record::scope::ARORA,
    author = "Semio",
    license = "Apache-2.0",
    description = "Keyframe animation, playback and baking over a nanosecond clock",
    executable_mime = "application/wasm"
)]
pub mod animation {
    use super::*;
    #[export(id = "76697a69-6a00-0000-0f00-000000000001")]
    pub fn load_animation(
        #[param(id = "76697a69-6a00-0000-0f01-000000000001")] clip: AnimationClip,
    ) -> u32 {
        guest(|a| AnimationContract::load_animation(a, clip))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000002")]
    pub fn create_player(
        #[param(id = "76697a69-6a00-0000-0f02-000000000001")] name: Option<String>,
    ) -> u32 {
        guest(|a| AnimationContract::create_player(a, name))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000003")]
    pub fn add_instance(
        #[param(id = "76697a69-6a00-0000-0f03-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f03-000000000002")] anim: u32,
    ) -> u32 {
        guest(|a| AnimationContract::add_instance(a, player, anim))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000004")]
    pub fn step(
        #[param(id = "76697a69-6a00-0000-0f04-000000000001")] dt_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f04-000000000002")] time_ns: Option<u64>,
    ) -> Vec<TrackOutput> {
        guest(|a| AnimationContract::step(a, dt_ns, time_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000005")]
    pub fn play(#[param(id = "76697a69-6a00-0000-0f05-000000000001")] player: u32) -> u32 {
        guest(|a| AnimationContract::play(a, player))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000006")]
    pub fn pause(#[param(id = "76697a69-6a00-0000-0f06-000000000001")] player: u32) -> u32 {
        guest(|a| AnimationContract::pause(a, player))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000007")]
    pub fn stop(#[param(id = "76697a69-6a00-0000-0f07-000000000001")] player: u32) -> u32 {
        guest(|a| AnimationContract::stop(a, player))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000008")]
    pub fn seek(
        #[param(id = "76697a69-6a00-0000-0f08-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f08-000000000002")] time_ns: u64,
    ) -> u32 {
        guest(|a| AnimationContract::seek(a, player, time_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000009")]
    pub fn set_speed(
        #[param(id = "76697a69-6a00-0000-0f09-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f09-000000000002")] speed: f32,
    ) -> u32 {
        guest(|a| AnimationContract::set_speed(a, player, speed))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000000a")]
    pub fn set_loop(
        #[param(id = "76697a69-6a00-0000-0f0a-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f0a-000000000002")] mode: String,
    ) -> u32 {
        guest(|a| AnimationContract::set_loop(a, player, mode))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000000b")]
    pub fn set_weight(
        #[param(id = "76697a69-6a00-0000-0f0b-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f0b-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f0b-000000000003")] weight: f32,
    ) -> u32 {
        guest(|a| AnimationContract::set_weight(a, player, instance, weight))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000000c")]
    pub fn remove_instance(
        #[param(id = "76697a69-6a00-0000-0f0c-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f0c-000000000002")] instance: u32,
    ) -> u32 {
        guest(|a| AnimationContract::remove_instance(a, player, instance))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000000d")]
    pub fn player_states() -> Vec<PlayerState> {
        guest(AnimationContract::player_states)
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000000e")]
    pub fn bake(
        #[param(id = "76697a69-6a00-0000-0f0e-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f0e-000000000002")] frame_rate: Option<f32>,
        #[param(id = "76697a69-6a00-0000-0f0e-000000000003")] start_ns: Option<u64>,
        #[param(id = "76697a69-6a00-0000-0f0e-000000000004")] end_ns: Option<u64>,
    ) -> Option<BakedAnimation> {
        guest(|a| AnimationContract::bake(a, anim, frame_rate, start_ns, end_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000000f")]
    pub fn bake_with_derivatives(
        #[param(id = "76697a69-6a00-0000-0f0f-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f0f-000000000002")] frame_rate: Option<f32>,
        #[param(id = "76697a69-6a00-0000-0f0f-000000000003")] start_ns: Option<u64>,
        #[param(id = "76697a69-6a00-0000-0f0f-000000000004")] end_ns: Option<u64>,
    ) -> Option<BakedAnimation> {
        guest(|a| AnimationContract::bake_with_derivatives(a, anim, frame_rate, start_ns, end_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000010")]
    pub fn unload_animation(
        #[param(id = "76697a69-6a00-0000-0f10-000000000001")] anim: u32,
    ) -> bool {
        guest(|a| AnimationContract::unload_animation(a, anim))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000011")]
    pub fn remove_player(
        #[param(id = "76697a69-6a00-0000-0f11-000000000001")] player: u32,
    ) -> bool {
        guest(|a| AnimationContract::remove_player(a, player))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000012")]
    pub fn add_instance_with_weight(
        #[param(id = "76697a69-6a00-0000-0f12-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f12-000000000002")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f12-000000000003")] weight: f32,
    ) -> u32 {
        guest(|a| AnimationContract::add_instance_with_weight(a, player, anim, weight))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000013")]
    pub fn reload_animation(
        #[param(id = "76697a69-6a00-0000-0f13-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f13-000000000002")] clip: AnimationClip,
    ) -> bool {
        guest(|a| AnimationContract::reload_animation(a, anim, clip))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000014")]
    pub fn set_window(
        #[param(id = "76697a69-6a00-0000-0f14-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f14-000000000002")] start_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f14-000000000003")] end_ns: Option<u64>,
    ) -> u32 {
        guest(|a| AnimationContract::set_window(a, player, start_ns, end_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000015")]
    pub fn play_at(
        #[param(id = "76697a69-6a00-0000-0f15-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f15-000000000002")] time_ns: u64,
    ) -> u32 {
        guest(|a| AnimationContract::play_at(a, player, time_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000016")]
    pub fn set_start_offset(
        #[param(id = "76697a69-6a00-0000-0f16-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f16-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f16-000000000003")] offset_ns: i64,
    ) -> u32 {
        guest(|a| AnimationContract::set_start_offset(a, player, instance, offset_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000017")]
    pub fn set_time_scale(
        #[param(id = "76697a69-6a00-0000-0f17-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f17-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f17-000000000003")] time_scale: f32,
    ) -> u32 {
        guest(|a| AnimationContract::set_time_scale(a, player, instance, time_scale))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000018")]
    pub fn step_values(
        #[param(id = "76697a69-6a00-0000-0f18-000000000001")] dt_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f18-000000000002")] time_ns: Option<u64>,
    ) -> StepValues {
        guest(|a| AnimationContract::step_values(a, dt_ns, time_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-000000000019")]
    pub fn output_keys() -> OutputKeys {
        guest(AnimationContract::output_keys)
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000001a")]
    pub fn load_clip(#[param(id = "76697a69-6a00-0000-0f1a-000000000001")] clip: Clip) -> u32 {
        guest(|a| AnimationContract::load_clip(a, clip))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000001b")]
    pub fn reload_clip(
        #[param(id = "76697a69-6a00-0000-0f1b-000000000001")] anim: u32,
        #[param(id = "76697a69-6a00-0000-0f1b-000000000002")] clip: Clip,
    ) -> bool {
        guest(|a| AnimationContract::reload_clip(a, anim, clip))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000001c")]
    pub fn set_rate(
        #[param(id = "76697a69-6a00-0000-0f1c-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f1c-000000000002")] instance: u32,
        #[param(id = "76697a69-6a00-0000-0f1c-000000000003")] rate: f64,
    ) -> u32 {
        guest(|a| AnimationContract::set_rate(a, player, instance, rate))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000001d")]
    pub fn step_with_velocities(
        #[param(id = "76697a69-6a00-0000-0f1d-000000000001")] dt_ns: u64,
        #[param(id = "76697a69-6a00-0000-0f1d-000000000002")] time_ns: Option<u64>,
    ) -> StepSamples {
        guest(|a| AnimationContract::step_with_velocities(a, dt_ns, time_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000001e")]
    pub fn set_length(
        #[param(id = "76697a69-6a00-0000-0f1e-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f1e-000000000002")] length_ns: Option<u64>,
    ) -> u32 {
        guest(|a| AnimationContract::set_length(a, player, length_ns))
    }
    #[export(id = "76697a69-6a00-0000-0f00-00000000001f")]
    pub fn set_reversed(
        #[param(id = "76697a69-6a00-0000-0f1f-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f1f-000000000002")] reversed: bool,
    ) -> u32 {
        guest(|a| AnimationContract::set_reversed(a, player, reversed))
    }
}
