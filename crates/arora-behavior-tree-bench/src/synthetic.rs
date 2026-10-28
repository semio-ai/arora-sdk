//! A public, deterministic workload with graph sinks and event-driven branches.
use crate::model::Workload;
use serde_json::{json, Value};
pub const TICKS: usize = 160;
pub const WARM_UP: usize = 16;

pub fn json() -> Value {
    let mut text = String::from("\
parallel
  fallback
    sequence
      equal(a: {mode}, b: 0f64)
      parallel
        {target} = add(operand_0: @mixed)
        {branch/output} = add(operand_0: @n17)
        running
    sequence
      equal(a: {mode}, b: 1f64)
      parallel
        pulse(t: {arora/time}, gap_min: 0.04, gap_max: 0.10, low: 0.2, high: 0.8, width: 0.025, salt: 7, out: {target}, state: ())
        wander(t: {arora/time}, gap_min: 0.05, gap_max: 0.08, salt: 9, low: -0.4, high: 0.4, out: {branch/output}, state: ())
    sequence
      equal(a: {mode}, b: 2f64)
      parallel
        hold(t: {arora/time}, duration: 0, value: 0.6, out: {target}, state: ())
        unpack(values: @frames, out_0: {branch/output})
    sequence
      equal(a: {mode}, b: 3f64)
      parallel
        unpack(values: f32[-0.25], out_0: {target})
        equal(a: @above, b: true)
        {branch/output} = add(operand_0: @n17)
  running
  say(text: {speech/text}, voice: \"\", viseme: {speech/viseme})

input_sum: add(node: \"input_sum\", operand_0: {input}, operand_1: 0.3f32)
gain: multiply(node: \"gain\", operand_0: @input_sum, operand_1: 0.8f32)
wave: oscillator(node: \"wave\", frequency: 2f32, phase: 0.1f32, t: {arora/time})
mixed: add(node: \"mixed\", operand_0: @gain, operand_1: @wave)
damped: {graph/damped} = damp(node: \"damped\", in: {target}, param.half_life: 0.12f32, dt: 0.016666667f32, state: ())
output: {graph/mixed} = add(node: \"output\", operand_0: @damped, operand_1: @gain)
above: greaterthan(node: \"above\", lhs: {input}, rhs: -1f32)
frame_time: greaterthan(node: \"frame_time\", lhs: {arora/time}, rhs: 1.55f32)
frames: if(node: \"frames\", cond: @frame_time, then: f32[0.9], else: f32[0.2])
");
    for i in 6..18 {
        let previous = if i == 6 {
            "mixed".to_string()
        } else {
            format!("n{}", i - 1)
        };
        text.push_str(&format!(
            "n{i}: add(node: \"n{i}\", operand_0: @{previous}, operand_1: 0.02f32)\n"
        ));
    }
    let mut events: Vec<_> = (0..TICKS)
        .flat_map(|tick| {
            [
                json!([tick, "input", ((tick / 7) % 11) as f64 / 10.0]),
                json!([tick, "mode", tick / 40]),
            ]
        })
        .collect();
    events.extend([
        json!([5, "speech/text", "Hi."]),
        json!([30, "speech/text", "This sentence is interrupted."]),
        json!([40, "speech/text", "Done."]),
        json!([80, "speech/text", ""]),
        json!([100, "speech/text", "Hello."]),
    ]);
    json!({"dt":1.0/60.0,"ticks":TICKS,"warmUp":WARM_UP,"clock":"arora/time",
        "keys":[{"name":"arora/time","initial":0.0},{"name":"input","initial":0.0},{"name":"mode","initial":0.0},
            {"name":"target","initial":0.0},{"name":"branch/output","initial":0.0},
            {"name":"graph/damped","initial":null},{"name":"graph/mixed","initial":null},
            {"name":"speech/text","initial":""},{"name":"speech/viseme","initial":""}],
        "text":text,"events":events,
        "phases":{"names":["follow","pulse","hold","set"],"runs":[[0,0],[40,1],[80,2],[120,3]]}})
}
pub fn workload() -> Workload {
    Workload::parse(&json().to_string()).expect("valid synthetic workload")
}
