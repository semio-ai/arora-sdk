//! What a learned control policy needs to run inside an Arora module.
//!
//! A policy is a function from an observation vector to an action vector,
//! evaluated at a fixed rate. This crate provides the three things every such
//! module reimplements otherwise, robot-agnostic and free of any host
//! dependency so the module builds for `wasm32-wasip1` unchanged:
//!
//! - [`OnnxPolicy`]: the network itself, loaded from ONNX bytes the module
//!   embeds (`include_bytes!`) and evaluated with [tract](https://github.com/sonos/tract),
//!   a pure-Rust inference engine. Any exporter that writes a plain
//!   feed-forward ONNX graph (PyTorch, JAX via `jax2onnx`, …) produces a file
//!   this loads.
//! - [`History`]: the fixed-length buffer of past vectors that most locomotion
//!   policies take as input (the last N actions, the last N observations).
//! - [`Decimator`]: the clock that turns the runtime's variable tick period
//!   into the policy's fixed control period, so the module infers at the rate
//!   it was trained for whatever rate the behavior ticks it at.
//!
//! Plus the small geometry the observation usually needs
//! ([`projected_gravity`]).

use std::sync::Arc;
use tract_onnx::prelude::*;
use tract_onnx::tract_hir::internal::DimLike;

/// A policy network loaded from ONNX, ready to evaluate.
///
/// The graph is expected to take one input of shape `[1, N]` (or `[N]`) and
/// to produce one output of shape `[1, M]` (or `[M]`), both `f32` — the shape
/// every actor network exported for deployment has. Batch dimensions are
/// squeezed away on both sides.
pub struct OnnxPolicy {
    plan: Arc<TypedSimplePlan>,
    input_len: usize,
    output_len: usize,
}

/// A policy could not be loaded or evaluated.
#[derive(Debug)]
pub struct PolicyError {
    pub message: String,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PolicyError {}

impl From<tract_onnx::prelude::TractError> for PolicyError {
    fn from(error: tract_onnx::prelude::TractError) -> Self {
        Self {
            message: format!("{error:?}"),
        }
    }
}

impl OnnxPolicy {
    /// Load a policy from the bytes of an ONNX file.
    ///
    /// The model is type-checked, optimized and compiled to an execution
    /// plan once here; [`infer`](Self::infer) then only runs it. Loading a
    /// policy of a few hundred kilobytes takes milliseconds natively and well
    /// under a second in a wasm guest, so a module loads at its first call and
    /// keeps the result.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PolicyError> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(bytes))?
            .into_optimized()?;
        let input_len = flat_len(&model, model.input_outlets()?.first().copied())?;
        let output_len = flat_len(&model, model.output_outlets()?.first().copied())?;
        let plan = model.into_runnable()?;
        Ok(Self {
            plan,
            input_len,
            output_len,
        })
    }

    /// The observation length the network takes.
    pub fn input_len(&self) -> usize {
        self.input_len
    }

    /// The action length the network produces.
    pub fn output_len(&self) -> usize {
        self.output_len
    }

    /// Evaluate the network on one observation.
    ///
    /// `observation` must be exactly [`input_len`](Self::input_len) long — a
    /// mismatch is refused rather than padded, since it means the observation
    /// layout and the network disagree, which no amount of running can fix.
    pub fn infer(&self, observation: &[f32]) -> Result<Vec<f32>, PolicyError> {
        if observation.len() != self.input_len {
            return Err(PolicyError {
                message: format!(
                    "the policy takes {} observation values, got {}",
                    self.input_len,
                    observation.len()
                ),
            });
        }
        let input = tract_ndarray::Array2::from_shape_vec((1, observation.len()), observation.to_vec())
            .map_err(|e| PolicyError {
                message: e.to_string(),
            })?;
        let outputs = self.plan.run(tvec!(Tensor::from(input).into()))?;
        let output = outputs.first().ok_or_else(|| PolicyError {
            message: "the policy produced no output".to_string(),
        })?;
        Ok(output.try_as_plain_ram()?.as_slice::<f32>()?.to_vec())
    }
}

/// The number of elements of a model's outlet, its batch dimension included
/// (a deployment export has a batch of one, so this is the flat length).
fn flat_len(model: &TypedModel, outlet: Option<OutletId>) -> Result<usize, PolicyError> {
    let outlet = outlet.ok_or_else(|| PolicyError {
        message: "the policy graph has no input/output".to_string(),
    })?;
    let fact = model.outlet_fact(outlet)?;
    let shape: Vec<usize> = fact
        .shape
        .iter()
        .map(|dim| {
            dim.to_usize().map_err(|_| PolicyError {
                message: format!("the policy has a symbolic dimension ({dim}); export it with a batch of one"),
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(shape.iter().product())
}

/// A fixed-length window of the most recent vectors, flattened oldest first —
/// the "last N actions" / "last N observations" input most policies take.
///
/// It starts full of zeros, which is also what a fresh episode looks like in
/// training, so a module resets a history by constructing it again.
#[derive(Debug, Clone)]
pub struct History {
    width: usize,
    depth: usize,
    /// `depth * width` values, oldest first.
    values: Vec<f32>,
}

impl History {
    /// A window of `depth` vectors of `width` values each, all zero.
    pub fn new(width: usize, depth: usize) -> Self {
        Self {
            width,
            depth,
            values: vec![0.0; width * depth],
        }
    }

    /// Push the newest vector, dropping the oldest.
    ///
    /// `vector` must be `width` long; a mismatch is a layout bug and panics.
    pub fn push(&mut self, vector: &[f32]) {
        assert_eq!(
            vector.len(),
            self.width,
            "a history of {}-wide vectors was pushed a {}-wide one",
            self.width,
            vector.len()
        );
        if self.depth == 0 {
            return;
        }
        self.values.copy_within(self.width.., 0);
        let start = (self.depth - 1) * self.width;
        self.values[start..].copy_from_slice(vector);
    }

    /// The window, oldest vector first, newest last.
    pub fn flattened(&self) -> &[f32] {
        &self.values
    }

    /// The most recent vector (zeros before the first push).
    pub fn newest(&self) -> &[f32] {
        if self.depth == 0 {
            return &[];
        }
        &self.values[(self.depth - 1) * self.width..]
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}

/// Turns the runtime's tick period into a policy's fixed control period.
///
/// The Arora runtime ticks a behavior at its own rate (100 Hz by default) and
/// publishes the elapsed time as `arora/dt`; a policy was trained to act at a
/// fixed rate (50 Hz, say). Feed every tick's `dt` here and act only when
/// [`due`](Self::due) says so: it accumulates the elapsed time and fires once
/// per control period, catching up at most one period if the runtime stalled
/// (an inference is worth running on stale time, a burst of them is not).
#[derive(Debug, Clone)]
pub struct Decimator {
    period_ns: u64,
    accumulated_ns: u64,
}

impl Decimator {
    /// A decimator firing every `period_ns` nanoseconds, due at the first
    /// tick so the policy acts as soon as it is started.
    pub fn new(period_ns: u64) -> Self {
        Self {
            period_ns,
            accumulated_ns: period_ns,
        }
    }

    /// The control period, in nanoseconds.
    pub fn period_ns(&self) -> u64 {
        self.period_ns
    }

    /// Account for `dt_ns` more nanoseconds; `true` when a control period has
    /// elapsed since the last firing.
    pub fn due(&mut self, dt_ns: u64) -> bool {
        self.accumulated_ns = self.accumulated_ns.saturating_add(dt_ns);
        if self.accumulated_ns >= self.period_ns {
            // Keep the remainder so the rate stays exact, but never more than
            // one period: a stall must not turn into a burst.
            self.accumulated_ns = (self.accumulated_ns - self.period_ns).min(self.period_ns);
            true
        } else {
            false
        }
    }

    /// Fire at the next tick, whatever time elapsed.
    pub fn reset(&mut self) {
        self.accumulated_ns = self.period_ns;
    }
}

/// The gravity direction expressed in the body frame — the "which way is
/// down" input a balancing policy takes — from the body's orientation as a
/// unit quaternion `[w, x, y, z]` (the MuJoCo and ROS scalar-first
/// convention, world-from-body).
///
/// Returns the unit vector pointing down in the body frame: `[0, 0, -1]` for
/// an upright body, tilting toward the side that drops.
pub fn projected_gravity(quaternion_wxyz: [f32; 4]) -> [f32; 3] {
    let [w, x, y, z] = quaternion_wxyz;
    // Rotate the world down vector (0, 0, -1) by the inverse of the body's
    // orientation: q⁻¹ · (0,0,-1) · q, written out for that fixed vector.
    [
        -2.0 * (x * z - w * y),
        -2.0 * (y * z + w * x),
        -(1.0 - 2.0 * (x * x + y * y)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_zero_then_oldest_first() {
        let mut h = History::new(2, 3);
        assert_eq!(h.flattened(), &[0.0; 6]);
        h.push(&[1.0, 1.5]);
        h.push(&[2.0, 2.5]);
        assert_eq!(h.flattened(), &[0.0, 0.0, 1.0, 1.5, 2.0, 2.5]);
        assert_eq!(h.newest(), &[2.0, 2.5]);
        h.push(&[3.0, 3.5]);
        h.push(&[4.0, 4.5]);
        assert_eq!(h.flattened(), &[2.0, 2.5, 3.0, 3.5, 4.0, 4.5]);
    }

    #[test]
    fn decimator_fires_at_the_control_rate_from_faster_ticks() {
        let mut d = Decimator::new(20_000_000); // 50 Hz
        // The first tick fires immediately; its 10 ms already count toward
        // the next period, so the next firing is one more tick later, then
        // every second tick.
        assert!(d.due(10_000_000));
        let fired: Vec<bool> = (0..6).map(|_| d.due(10_000_000)).collect();
        assert_eq!(fired, [true, false, true, false, true, false]);
    }

    #[test]
    fn decimator_catches_up_one_period_at_most() {
        let mut d = Decimator::new(20_000_000);
        assert!(d.due(0));
        // A 100 ms stall: one firing now, one more next tick, then back to rate.
        assert!(d.due(100_000_000));
        assert!(d.due(0));
        assert!(!d.due(0));
    }

    #[test]
    fn gravity_projects_down_for_an_upright_body_and_tilts_with_it() {
        let upright = projected_gravity([1.0, 0.0, 0.0, 0.0]);
        assert!((upright[0]).abs() < 1e-6 && (upright[1]).abs() < 1e-6 && (upright[2] + 1.0).abs() < 1e-6);
        // Pitched 90° nose-down about +y: the body's x axis points down, so
        // gravity appears along +x in the body frame.
        let half = std::f32::consts::FRAC_PI_4;
        let pitched = projected_gravity([half.cos(), 0.0, half.sin(), 0.0]);
        assert!((pitched[0] - 1.0).abs() < 1e-5, "{pitched:?}");
        assert!(pitched[2].abs() < 1e-5, "{pitched:?}");
    }
}
