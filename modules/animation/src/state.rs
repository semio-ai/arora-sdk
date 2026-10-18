use crate::{convert::value, *};
use arora_animation::{
    clip1::ClipV1, engine::Output, AnimId, Engine, InstId, LoopMode, PlayerId, Timing,
};
use arora_types::value::Value;
use std::collections::HashMap;

/// One module load's animation state. A host owns one per device.
#[derive(Default)]
pub struct AnimationModule {
    engine: Engine,
    clock: Option<i64>,
    // Bakes preserve 1.x number widths too; the engine's bake returns scalars.
    narrow: HashMap<u32, Vec<bool>>,
}

/// A time the interface carries unsigned, in the engine's signed
/// nanoseconds: one past what `i64` holds (292 years) is held at its end.
fn ns(time: u64) -> i64 {
    i64::try_from(time).unwrap_or(i64::MAX)
}

fn result(id: u32, accepted: bool) -> u32 {
    if accepted {
        id
    } else {
        u32::MAX
    }
}

impl AnimationModule {
    pub fn new() -> Self {
        Self::default()
    }

    fn advance(&mut self, dt_ns: u64, time_ns: Option<u64>) -> Vec<Output> {
        let dt = ns(dt_ns);
        let now = time_ns.map_or_else(|| self.clock.unwrap_or(0).saturating_add(dt), ns);
        self.clock = Some(now);
        self.engine.step(now, dt)
    }

    fn timing(&mut self, player: u32, instance: u32, change: impl FnOnce(&mut Timing)) -> u32 {
        let Some(i) = self.engine.instance(PlayerId(player), InstId(instance)) else {
            return u32::MAX;
        };
        let mut timing = i.timing;
        change(&mut timing);
        result(
            instance,
            self.engine
                .set_timing(PlayerId(player), InstId(instance), timing),
        )
    }

    fn load(&mut self, clip: arora_animation::Clip, narrow: Vec<bool>) -> u32 {
        if clip.validate().is_err() {
            return u32::MAX;
        }
        let id = self.engine.load(clip, narrow.clone()).0;
        self.narrow.insert(id, narrow);
        id
    }

    fn reload(&mut self, anim: u32, clip: arora_animation::Clip, narrow: Vec<bool>) -> bool {
        if clip.validate().is_err() || !self.engine.reload(AnimId(anim), clip, narrow.clone()) {
            return false;
        }
        self.narrow.insert(anim, narrow);
        true
    }

    fn baked(
        &self,
        anim: u32,
        frame_rate: Option<f32>,
        start_ns: Option<u64>,
        end_ns: Option<u64>,
        derivatives: bool,
    ) -> Option<BakedAnimation> {
        let baked = self.engine.bake(
            AnimId(anim),
            frame_rate.map_or(60.0, f64::from),
            start_ns.map(ns),
            end_ns.map(ns),
        )?;
        let narrow = self.narrow.get(&anim);
        Some(BakedAnimation {
            frame_rate: baked.rate as f32,
            start_ns: baked.start.max(0) as u64,
            end_ns: baked.end.max(0) as u64,
            tracks: baked
                .tracks
                .into_iter()
                .enumerate()
                .map(|(i, (target, samples))| {
                    let narrow = narrow.and_then(|v| v.get(i)).copied().unwrap_or(false);
                    BakedTrack {
                        animatable_id: target,
                        derivatives: Value::ArrayValue(if derivatives {
                            samples
                                .iter()
                                .map(|s| Value::Option(s.velocity.map(|v| Box::new(Value::F64(v)))))
                                .collect()
                        } else {
                            vec![]
                        }),
                        values: Value::ArrayValue(
                            samples
                                .into_iter()
                                .map(|s| value(s.value, narrow))
                                .collect(),
                        ),
                    }
                })
                .collect(),
        })
    }
}

impl AnimationContract for AnimationModule {
    fn load_animation(&mut self, clip: AnimationClip) -> u32 {
        let Some((clip, narrow)) = ClipV1::try_from(clip).ok().and_then(|c| c.to_clip().ok())
        else {
            return u32::MAX;
        };
        self.load(clip, narrow)
    }
    fn reload_animation(&mut self, anim: u32, clip: AnimationClip) -> bool {
        let Some((clip, narrow)) = ClipV1::try_from(clip).ok().and_then(|c| c.to_clip().ok())
        else {
            return false;
        };
        self.reload(anim, clip, narrow)
    }
    fn load_clip(&mut self, clip: Clip) -> u32 {
        match arora_animation::Clip::try_from(clip) {
            Ok(clip) => self.load(clip, vec![]),
            Err(_) => u32::MAX,
        }
    }
    fn reload_clip(&mut self, anim: u32, clip: Clip) -> bool {
        match arora_animation::Clip::try_from(clip) {
            Ok(clip) => self.reload(anim, clip, vec![]),
            Err(_) => false,
        }
    }
    fn create_player(&mut self, name: Option<String>) -> u32 {
        let player = self.engine.create_player(&name.unwrap_or_default());
        self.engine.set_mode(player, LoopMode::Loop);
        self.engine.play(player);
        player.0
    }
    fn add_instance(&mut self, player: u32, anim: u32) -> u32 {
        self.add_instance_with_weight(player, anim, 1.0)
    }
    fn add_instance_with_weight(&mut self, player: u32, anim: u32, weight: f32) -> u32 {
        self.engine
            .add_instance(
                PlayerId(player),
                AnimId(anim),
                weight.into(),
                Timing::default(),
            )
            .map_or(u32::MAX, |i| i.0)
    }
    fn remove_instance(&mut self, player: u32, instance: u32) -> u32 {
        u32::from(
            self.engine
                .remove_instance(PlayerId(player), InstId(instance)),
        )
    }
    fn remove_player(&mut self, player: u32) -> bool {
        self.engine.remove_player(PlayerId(player))
    }
    fn unload_animation(&mut self, anim: u32) -> bool {
        self.narrow.remove(&anim);
        self.engine.unload(AnimId(anim))
    }
    fn play(&mut self, player: u32) -> u32 {
        result(player, self.engine.play(PlayerId(player)))
    }
    fn play_at(&mut self, player: u32, time_ns: u64) -> u32 {
        result(player, self.engine.play_at(PlayerId(player), ns(time_ns)))
    }
    fn pause(&mut self, player: u32) -> u32 {
        result(player, self.engine.pause(PlayerId(player)))
    }
    fn stop(&mut self, player: u32) -> u32 {
        result(player, self.engine.stop(PlayerId(player)))
    }
    fn seek(&mut self, player: u32, time_ns: u64) -> u32 {
        result(player, self.engine.seek(PlayerId(player), ns(time_ns)))
    }
    fn set_speed(&mut self, player: u32, speed: f32) -> u32 {
        result(
            player,
            self.engine.set_speed(PlayerId(player), speed.into()),
        )
    }
    fn set_loop(&mut self, player: u32, mode: String) -> u32 {
        result(
            player,
            LoopMode::named(&mode).is_some_and(|m| self.engine.set_mode(PlayerId(player), m)),
        )
    }
    fn set_window(&mut self, player: u32, start_ns: u64, end_ns: Option<u64>) -> u32 {
        result(
            player,
            self.engine
                .set_window(PlayerId(player), ns(start_ns), end_ns.map(ns)),
        )
    }
    fn set_weight(&mut self, player: u32, instance: u32, weight: f32) -> u32 {
        result(
            instance,
            self.engine
                .set_weight(PlayerId(player), InstId(instance), weight.into()),
        )
    }
    fn set_start_offset(&mut self, player: u32, instance: u32, offset_ns: i64) -> u32 {
        self.timing(player, instance, |t| t.offset = offset_ns)
    }
    fn set_time_scale(&mut self, player: u32, instance: u32, time_scale: f32) -> u32 {
        if !time_scale.is_finite() || time_scale <= 0.0 {
            return u32::MAX;
        }
        self.set_rate(player, instance, 1.0 / f64::from(time_scale))
    }
    fn set_rate(&mut self, player: u32, instance: u32, rate: f64) -> u32 {
        self.timing(player, instance, |t| t.rate = rate)
    }
    fn step(&mut self, dt_ns: u64, time_ns: Option<u64>) -> Vec<TrackOutput> {
        let outputs = self.advance(dt_ns, time_ns);
        let (_, keys) = self.engine.output_keys();
        outputs
            .into_iter()
            .enumerate()
            .filter_map(|(i, o)| {
                Some(TrackOutput {
                    track_id: self.engine.output_tracks()[i].clone(),
                    default_key: keys[i].clone(),
                    value: value(o.sample?.value, o.narrow),
                })
            })
            .collect()
    }
    fn step_values(&mut self, dt_ns: u64, time_ns: Option<u64>) -> StepValues {
        let outputs = self.advance(dt_ns, time_ns);
        StepValues {
            revision: self.engine.output_keys().0,
            values: Value::ArrayValue(
                outputs
                    .into_iter()
                    .map(|o| o.sample.map_or(Value::Unit, |s| value(s.value, o.narrow)))
                    .collect(),
            ),
        }
    }
    fn step_with_velocities(&mut self, dt_ns: u64, time_ns: Option<u64>) -> StepSamples {
        let outputs = self.advance(dt_ns, time_ns);
        StepSamples {
            revision: self.engine.output_keys().0,
            velocities: Value::ArrayValue(
                outputs
                    .iter()
                    .map(|o| {
                        o.sample
                            .as_ref()
                            .and_then(|s| s.velocity)
                            .map_or(Value::Unit, Value::F64)
                    })
                    .collect(),
            ),
            values: Value::ArrayValue(
                outputs
                    .into_iter()
                    .map(|o| o.sample.map_or(Value::Unit, |s| value(s.value, o.narrow)))
                    .collect(),
            ),
        }
    }
    fn set_length(&mut self, player: u32, length_ns: Option<u64>) -> u32 {
        result(
            player,
            self.engine.set_length(PlayerId(player), length_ns.map(ns)),
        )
    }
    fn output_keys(&mut self) -> OutputKeys {
        let (revision, keys) = self.engine.output_keys();
        OutputKeys {
            revision,
            keys: keys.to_vec(),
        }
    }
    fn player_states(&mut self) -> Vec<PlayerState> {
        self.engine
            .players()
            .into_iter()
            .map(|p| PlayerState {
                player: p.id.0,
                name: p.name,
                state: p.state.name().into(),
                time_ns: p.position.max(0) as u64,
                duration_ns: p.length.max(0) as u64,
                speed: p.speed as f32,
                reversed: p.reversed,
                loop_mode: p.mode.name().into(),
                ended: p.ended,
                window_start_ns: p.window.0.max(0) as u64,
                window_end_ns: p.window.1.map(|n| n.max(0) as u64),
                instances: p
                    .instances
                    .into_iter()
                    .map(|i| InstanceState {
                        instance: i.id.0,
                        anim: i.anim.0,
                        weight: i.weight as f32,
                        start_offset_ns: i.timing.offset,
                        time_scale: (1.0 / i.timing.rate) as f32,
                        rate: i.timing.rate,
                    })
                    .collect(),
            })
            .collect()
    }
    fn bake(
        &mut self,
        anim: u32,
        frame_rate: Option<f32>,
        start_ns: Option<u64>,
        end_ns: Option<u64>,
    ) -> Option<BakedAnimation> {
        self.baked(anim, frame_rate, start_ns, end_ns, false)
    }
    fn bake_with_derivatives(
        &mut self,
        anim: u32,
        frame_rate: Option<f32>,
        start_ns: Option<u64>,
        end_ns: Option<u64>,
    ) -> Option<BakedAnimation> {
        self.baked(anim, frame_rate, start_ns, end_ns, true)
    }
}
