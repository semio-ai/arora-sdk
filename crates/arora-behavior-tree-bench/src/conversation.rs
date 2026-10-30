//! Deterministic social conversation, with neutral actuator keys and no robot assets.
use crate::model::Workload;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const TICKS: usize = 90 * 60;
pub const WARM_UP: usize = 60;
const DT: f64 = 1.0 / 60.0;
const EMOTIONS: [&str; 6] = ["concerned", "happy", "sad", "sleepy", "surprise", "anger"];
const JOINTS: [(&str, f64, f64, f64); 8] = [
    ("arm_left_lift", -0.2, 1.6, 0.004),
    ("arm_left_flex", 0.0, 1.8, 0.004),
    ("arm_right_lift", -0.2, 1.6, 0.004),
    ("arm_right_flex", 0.0, 1.8, 0.004),
    ("neck_pitch", -0.5, 0.4, 0.006),
    ("neck_yaw", -0.9, 0.9, 0.0),
    ("trunk_yaw", -0.6, 0.6, 0.0),
    ("trunk_pitch", -0.2, 0.2, 0.02),
];
fn key(name: &str) -> String {
    format!("{{{name}}}")
}
fn number(x: f64) -> String {
    format!("{x}f32")
}
fn joint(i: usize) -> String {
    format!("body/joint/{}/target", JOINTS[i].0)
}
fn beat(i: usize) -> String {
    format!("body/joint/{}/beat", JOINTS[i].0)
}
fn group(kind: &str, children: Vec<String>) -> String {
    format!(
        "{kind}\n{}",
        children
            .iter()
            .map(|c| c.lines().map(|l| format!("  {l}\n")).collect::<String>())
            .collect::<String>()
    )
    .trim_end()
    .into()
}
fn seq(children: Vec<String>) -> String {
    group("sequence", children)
}
fn par(children: Vec<String>) -> String {
    group("parallel", children)
}
fn fallback(children: Vec<String>) -> String {
    group("fallback", children)
}
fn equal(name: &str, value: f64) -> String {
    format!("equal(a: {}, b: {value}f64)", key(name))
}
fn set(outputs: &[String], values: &[f64]) -> String {
    format!(
        "unpack(values: f64[{}], {})",
        values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        outputs
            .iter()
            .enumerate()
            .map(|(i, k)| format!("out_{i}: {}", key(k)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
fn publish(outputs: &[String], values: &[String]) -> String {
    par(outputs
        .iter()
        .zip(values)
        .map(|(k, v)| format!("{} = add(operand_0: {v})", key(k)))
        .chain(["running".into()])
        .collect())
}
fn hold(output: &str, value: &str, duration: &str) -> String {
    format!(
        "hold(t: {{arora/time}}, duration: {duration}, value: {value}, out: {}, state: ())",
        key(output)
    )
}
fn pulse(output: &str, args: &str) -> String {
    format!(
        "pulse(t: {{arora/time}}, {args}, out: {}, state: ())",
        key(output)
    )
}
fn say(text: &str) -> String {
    format!("say(text: {text}, voice: \"\", viseme: {{speech/viseme}})")
}

#[derive(Default)]
struct Builder {
    graph: String,
    serial: usize,
}
impl Builder {
    fn call(&mut self, op: &str, args: &[(&str, String)], sink: Option<&str>) -> String {
        let id = format!("g{}", self.serial);
        self.serial += 1;
        self.graph += &format!(
            "{id}: {}{op}(node: \"{id}\", {})\n",
            sink.map(|k| format!("{} = ", key(k))).unwrap_or_default(),
            args.iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        format!("@{id}")
    }
    fn variadic(&mut self, op: &str, values: &[String], sink: Option<&str>) -> String {
        let names: Vec<_> = (0..values.len()).map(|i| format!("operand_{i}")).collect();
        self.call(
            op,
            &names
                .iter()
                .zip(values)
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect::<Vec<_>>(),
            sink,
        )
    }
    fn add(&mut self, values: &[String], sink: Option<&str>) -> String {
        self.variadic("add", values, sink)
    }
    fn mul(&mut self, values: &[String]) -> String {
        self.variadic("multiply", values, None)
    }
    fn sub(&mut self, a: &str, b: &str, sink: Option<&str>) -> String {
        self.call("subtract", &[("lhs", a.into()), ("rhs", b.into())], sink)
    }
    fn damp(&mut self, input: &str, half: f64, sink: Option<&str>) -> String {
        self.call(
            "damp",
            &[
                ("in", input.into()),
                ("param.half_life", number(half)),
                ("dt", number(DT)),
                ("state", "()".into()),
            ],
            sink,
        )
    }
    fn clamp(&mut self, input: &str, lo: f64, hi: f64, sink: Option<&str>) -> String {
        self.call(
            "clamp",
            &[
                ("in", input.into()),
                ("min", number(lo)),
                ("max", number(hi)),
            ],
            sink,
        )
    }
    fn remap(&mut self, input: &str, lo: f64, hi: f64, out_lo: f64, out_hi: f64) -> String {
        self.call(
            "remap",
            &[
                ("in", input.into()),
                ("in_min", number(lo)),
                ("in_max", number(hi)),
                ("out_min", number(out_lo)),
                ("out_max", number(out_hi)),
            ],
            None,
        )
    }
    fn osc(&mut self, f: f64) -> String {
        self.call(
            "oscillator",
            &[
                ("frequency", number(f)),
                ("phase", number(0.0)),
                ("t", key("arora/time")),
            ],
            None,
        )
    }
    fn noise(&mut self, f: f64, salt: f64) -> String {
        let x = self.mul(&[key("arora/time"), number(f)]);
        self.call("simplenoise", &[("x", x), ("y", number(salt))], None)
    }
    fn above(&mut self, input: &str, threshold: f64) -> String {
        let c = self.call(
            "greaterthan",
            &[("lhs", input.into()), ("rhs", number(threshold))],
            None,
        );
        format!("equal(a: {c}, b: true)")
    }
}

/// Frame selection is attached and ordered after the entry timer. Its private
/// start time cannot be read by a detached graph before that timer writes it.
fn frames(name: &str, outputs: &[String], frames: &[(f64, Vec<f64>)]) -> String {
    let start = format!(".{name}/start");
    let elapsed = format!(".{name}/elapsed");
    let values = |index: usize, values: &[f64]| {
        format!(
            "f64[{index},{}]",
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let mut selection = values(0, &frames[0].1);
    let mut steps = vec![format!(
        "{} = subtract(node: \"{name}/elapsed\", lhs: {{arora/time}}, rhs: {})",
        key(&elapsed),
        key(&start)
    )];
    for (i, (time, frame)) in frames.iter().enumerate().skip(1) {
        let cond = format!(".{name}/due{i}");
        let selected = format!(".{name}/frame{i}");
        // Tolerate f32 arithmetic at the exact boundary, within 1/1000 tick.
        steps.push(format!(
            "{} = greaterthan(node: \"{name}/due{i}\", lhs: {}, rhs: {})",
            key(&cond),
            key(&elapsed),
            number(time - 0.00001)
        ));
        steps.push(format!(
            "{} = if(node: \"{name}/frame{i}\", cond: {}, then: {}, else: {selection})",
            key(&selected),
            key(&cond),
            values(i, frame)
        ));
        selection = key(&selected);
    }
    let candidate = format!(".{name}/candidate");
    let previous = format!(".{name}/previous");
    let seen_start = format!(".{name}/seen_start");
    steps.push(format!(
        "unpack(values: {selection}, out_0: {})",
        key(&candidate)
    ));
    steps.push(fallback(vec![
        seq(vec![
            format!("equal(a: {}, b: {})", key(&seen_start), key(&start)),
            format!("equal(a: {}, b: {})", key(&previous), key(&candidate)),
        ]),
        seq(vec![
            format!(
                "unpack(values: {selection}, out_0: {}, {})",
                key(&previous),
                outputs
                    .iter()
                    .enumerate()
                    .map(|(i, k)| format!("out_{}: {}", i + 1, key(k)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!(
                "{} = if(node: \"{name}/seen_start\", cond: true, then: {}, else: 0f64)",
                key(&seen_start),
                key(&start)
            ),
        ]),
    ]));
    par(vec![
        hold(
            &start,
            "{arora/time}",
            &frames.last().unwrap().0.to_string(),
        ),
        seq(steps),
    ])
}

pub fn json() -> Value {
    let mut keys = vec![];
    for (name, initial) in [
        ("arora/time", 0.0),
        ("audio/user_energy", 0.0),
        ("audio/model_energy", 0.0),
        ("agent/user_speaking", 0.0),
        ("agent/model_speaking", 0.0),
        ("agent/awaiting_response", 0.0),
        ("agent/tool", 0.0),
        ("agent/tool/x", 0.0),
        ("agent/tool/y", 0.0),
        ("agent/tool/hold", 1.0),
        ("agent/tool/length", 1.0),
        ("agent/stop_request", 0.0),
        ("pointer/active", 0.0),
        ("pointer/x", 0.5),
        ("pointer/y", 0.5),
        ("perception/present", 1.0),
        ("perception/speaker_azimuth", 0.0),
        ("perception/speaker_elevation", 0.0),
        ("perception/size", 0.12),
        ("perception/dock_bearing", 30.0),
        ("perception/dock_distance", 2.5),
        ("power/battery", 0.24),
        ("safety/estop", 0.0),
        ("face/gaze/x", 0.0),
        ("face/gaze/y", 0.0),
        ("face/blink", 0.0),
    ] {
        keys.push(json!({"name":name,"initial":initial}));
    }
    for name in ["agent/response_text", "speech/viseme"] {
        keys.push(json!({"name":name,"initial":""}));
    }
    for e in EMOTIONS {
        for prefix in ["agent/tool/emotion", "face/emotion"] {
            keys.push(json!({"name":format!("{prefix}/{e}"),"initial":0.0}));
        }
    }
    for i in 0..8 {
        keys.push(json!({"name":joint(i),"initial":0.0}));
        if i < 4 {
            keys.push(json!({"name":beat(i),"initial":0.0}));
        }
    }
    let axes: Vec<_> = ["vx", "vy", "vyaw"]
        .map(|a| format!("body/base/{a}/target"))
        .into();
    let colors: Vec<_> = ["r", "g", "b", "level"]
        .map(|c| format!("body/led/{c}/target"))
        .into();
    for name in axes.iter().chain(&colors) {
        keys.push(json!({"name":name,"initial":0.0}));
    }
    let mut g = Builder::default();
    let voice = g.damp("{audio/user_energy}", 0.06, Some("perception/voice_energy"));
    let v = g.sub(&voice, &number(0.08), None);
    let v = g.mul(&[v, number(6.0)]);
    let v = g.clamp(&v, 0.0, 1.0, None);
    let user_turn = g.variadic(
        "max",
        &[v, key("agent/user_speaking")],
        Some("perception/user_turn"),
    );
    let fast = g.damp(
        "{perception/present}",
        0.15,
        Some("perception/presence_fast"),
    );
    let slow = g.damp(
        "{perception/present}",
        1.2,
        Some("perception/presence_slow"),
    );
    let arrival = g.sub(&fast, &slow, Some("perception/arrival"));
    let az = g.damp(
        "{perception/speaker_azimuth}",
        0.15,
        Some("perception/azimuth"),
    );
    let el = g.damp(
        "{perception/speaker_elevation}",
        0.15,
        Some("perception/elevation"),
    );
    let x = g.remap(&az, -60.0, 60.0, -0.8, 0.8);
    let look_x = g.clamp(&x, -1.0, 1.0, Some("perception/look_x"));
    let y = g.remap(&el, -30.0, 30.0, -0.5, 0.5);
    let look_y = g.clamp(&y, -1.0, 1.0, Some("perception/look_y"));
    let turn = g.variadic(
        "multiply",
        &[look_x.clone(), number(0.5)],
        Some("perception/turn"),
    );
    let charge = g.damp("{power/battery}", 1.0, Some("power/battery_smoothed"));
    let low = g.sub(&number(0.15), &charge, Some("power/low"));
    let energy = g.damp("{audio/model_energy}", 0.05, Some("speech/energy"));
    let d = g.call(
        "divide",
        &[("lhs", number(0.12)), ("rhs", key("perception/size"))],
        None,
    );
    let d = g.clamp(&d, 0.3, 3.0, None);
    let distance = g.damp(&d, 0.3, Some("perception/distance"));
    let far = g.sub(&distance, &number(1.4), Some("perception/too_far"));
    let close = g.sub(&number(0.7), &distance, Some("perception/too_close"));
    let wave = g.osc(0.05);
    let scan = g.mul(&[wave, number(0.4)]);
    let think: Vec<_> = [(0.9, 0.3, -0.4), (0.7, 1.7, 0.3)]
        .map(|(f, s, b)| {
            let n = g.noise(f, s);
            let n = g.mul(&[n, number(0.08)]);
            g.add(&[n, number(b)], None)
        })
        .into();
    let e = g.sub(&energy, &number(0.2), None);
    let e = g.mul(&[e, number(2.5)]);
    let emphasis = g.clamp(&e, 0.0, 1.0, None);
    let beats: Vec<_> = (0..4)
        .map(|i| {
            let n = g.noise(2.3 + 0.4 * i as f64, 0.5 * i as f64);
            g.mul(&[n, emphasis.clone(), number(0.25)])
        })
        .collect();
    let a = g.sub(&distance, &number(1.0), None);
    let a = g.mul(&[a, number(0.4)]);
    let a = g.clamp(&a, 0.0, 0.3, None);
    let yaw = g.mul(&[look_x.clone(), number(0.8)]);
    let yaw = g.clamp(&yaw, -0.5, 0.5, None);
    let d = g.mul(&[key("perception/dock_distance"), number(0.2)]);
    let d = g.clamp(&d, 0.0, 0.25, None);
    let dy = g.mul(&[key("perception/dock_bearing"), number(0.01)]);
    let dy = g.clamp(&dy, -0.4, 0.4, None);
    let glow = g.mul(&[energy, number(1.5)]);
    let glow = g.clamp(&glow, 0.2, 1.0, None);
    let p = g.osc(1.2);
    let p = g.add(&[p, number(1.0)], None);
    let thinking_pulse = g.mul(&[p, number(0.5)]);
    let mx = g.remap("{pointer/x}", 0.0, 1.0, -1.0, 1.0);
    let mx = g.clamp(&mx, -1.0, 1.0, None);
    let my = g.remap("{pointer/y}", 0.0, 1.0, 0.6, -0.6);
    let my = g.clamp(&my, -1.0, 1.0, None);
    let breath = g.osc(0.22);
    for (i, (name, lo, hi, amp)) in JOINTS.iter().enumerate() {
        let mut parts = vec![g.damp(&key(&joint(i)), 0.15, None)];
        if i < 4 {
            parts.push(g.damp(&key(&beat(i)), 0.1, None));
        }
        if *amp != 0.0 {
            parts.push(g.mul(&[breath.clone(), number(*amp)]));
        }
        let sum = if parts.len() > 1 {
            g.add(&parts, None)
        } else {
            parts[0].clone()
        };
        g.clamp(&sum, *lo, *hi, Some(&format!("body/joint/{name}/position")));
    }
    for (axis, limit) in ["vx", "vy", "vyaw"].iter().zip([0.3, 0.2, 0.6]) {
        let v = g.damp(&key(&format!("body/base/{axis}/target")), 0.2, None);
        g.clamp(
            &v,
            -limit,
            limit,
            Some(&format!("body/base/{axis}/command")),
        );
    }
    for c in ["r", "g", "b", "level"] {
        let v = g.damp(&key(&format!("body/led/{c}/target")), 0.1, None);
        g.clamp(&v, 0.0, 1.0, Some(&format!("body/led/{c}/command")));
    }

    let gaze: Vec<String> = vec!["face/gaze/x".into(), "face/gaze/y".into()];
    let emotions: Vec<_> = EMOTIONS.map(|e| format!("face/emotion/{e}")).into();
    let mut tools = vec![
        seq(vec![
            equal("agent/tool", 1.0),
            par(vec![
                hold(&gaze[0], "{agent/tool/x}", "{agent/tool/hold}"),
                hold(&gaze[1], "{agent/tool/y}", "{agent/tool/hold}"),
            ]),
            set(&["agent/tool".into()], &[0.0]),
        ]),
        seq(vec![
            equal("agent/tool", 2.0),
            par(EMOTIONS
                .iter()
                .zip(&emotions)
                .map(|(e, k)| {
                    hold(
                        k,
                        &key(&format!("agent/tool/emotion/{e}")),
                        "{agent/tool/length}",
                    )
                })
                .collect()),
            set(&emotions, &[0.0; 6]),
            set(&["agent/tool".into()], &[0.0]),
        ]),
    ];
    for (tid, name, outputs, fs) in [
        (
            3,
            "nod",
            vec![joint(4)],
            vec![
                (0.0, vec![-0.3]),
                (0.25, vec![0.0]),
                (0.5, vec![-0.3]),
                (0.75, vec![0.0]),
            ],
        ),
        (
            4,
            "shake",
            vec![joint(5)],
            vec![
                (0.0, vec![0.3]),
                (0.2, vec![-0.3]),
                (0.4, vec![0.3]),
                (0.6, vec![-0.3]),
                (0.8, vec![0.0]),
            ],
        ),
        (
            5,
            "wave",
            vec![joint(2), joint(3)],
            vec![
                (0.0, vec![1.3, 0.8]),
                (0.3, vec![1.3, 1.4]),
                (0.6, vec![1.3, 0.8]),
                (0.9, vec![1.3, 1.4]),
                (1.4, vec![0.0, 0.0]),
            ],
        ),
        (
            6,
            "point",
            vec![joint(2), joint(3)],
            vec![(0.0, vec![0.9, 0.1]), (1.5, vec![0.0, 0.0])],
        ),
    ] {
        tools.push(seq(vec![
            equal("agent/tool", tid as f64),
            frames(name, &outputs, &fs),
            set(&["agent/tool".into()], &[0.0]),
        ]));
    }
    tools.push("succeed".into());
    let listening = seq(vec![
        g.above(&user_turn, 0.5),
        par(vec![
            publish(&gaze, &[look_x, look_y]),
            pulse(
                &joint(4),
                "gap_min: 2.6, gap_max: 3.4, low: -0.25, high: -0.2, width: 0.3, salt: 7",
            ),
            fallback(vec![
                seq(vec![
                    equal("agent/model_speaking", 1.0),
                    set(&["agent/stop_request".into()], &[1.0]),
                ]),
                set(&["agent/stop_request".into()], &[0.0]),
            ]),
        ]),
    ]);
    let speaking = seq(vec![
        equal("agent/model_speaking", 1.0),
        par(vec![
            say("{agent/response_text}"),
            frames(
                "cue",
                &["face/blink".into(), gaze[0].clone(), gaze[1].clone()],
                &[
                    (0.0, vec![0.9, -0.35, -0.25]),
                    (0.08, vec![0.0, -0.35, -0.25]),
                    (0.35, vec![0.0, 0.0, 0.0]),
                ],
            ),
            publish(&(0..4).map(beat).collect::<Vec<_>>(), &beats),
            fallback(tools),
        ]),
    ]);
    let thinking = seq(vec![
        equal("agent/awaiting_response", 1.0),
        publish(&gaze, &think),
    ]);
    let idle_glance = par(gaze.iter().zip([(-0.45, 0.45, 11), (-0.2, 0.35, 12)])
        .map(|(k, (lo, hi, salt))| format!(
            "wander(t: {{arora/time}}, gap_min: 1.6, gap_max: 4.2, salt: {salt}, low: {lo}, high: {hi}, out: {}, state: ())",
            key(k)
        )).collect());
    let idle = fallback(vec![
        seq(vec![
            equal("pointer/active", 1.0),
            publish(&gaze, &[mx, my]),
        ]),
        idle_glance,
    ]);
    let conversation = fallback(vec![listening, speaking, thinking, idle]);
    let safety = fallback(vec![
        seq(vec![
            equal("safety/estop", 1.0),
            par((0..8).map(|i| hold(&joint(i), "0", "0")).collect()),
        ]),
        seq(vec![g.above(&low, 0.0), say("\"Battery low.\"")]),
        "succeed".into(),
    ]);
    let attention = fallback(vec![
        seq(vec![
            g.above(&arrival, 0.3),
            frames(
                "greet",
                &[joint(2), joint(3), "face/emotion/happy".into()],
                &[
                    (0.0, vec![1.3, 0.8, 1.0]),
                    (0.5, vec![1.3, 1.4, 1.0]),
                    (1.0, vec![1.3, 0.8, 1.0]),
                    (1.5, vec![0.0, 0.0, 0.0]),
                ],
            ),
        ]),
        seq(vec![
            g.above(&fast, 0.5),
            publish(&[joint(6), joint(5)], &[turn.clone(), turn]),
        ]),
        publish(&[joint(6), joint(5)], &[scan.clone(), scan]),
    ]);
    let base = fallback(vec![
        seq(vec![equal("safety/estop", 1.0), set(&axes, &[0.0; 3])]),
        seq(vec![
            g.above(&low, 0.0),
            publish(&axes, &[d, number(0.0), dy]),
        ]),
        seq(vec![
            g.above(&fast, 0.5),
            g.above(&far, 0.0),
            publish(&axes, &[a, number(0.0), yaw]),
        ]),
        seq(vec![g.above(&close, 0.0), set(&axes, &[-0.15, 0.0, 0.0])]),
        set(&axes, &[0.0; 3]),
    ]);
    let led = fallback(vec![
        seq(vec![
            equal("safety/estop", 1.0),
            set(&colors, &[1.0, 0.0, 0.0, 1.0]),
        ]),
        seq(vec![
            g.above(&user_turn, 0.5),
            set(&colors, &[0.1, 0.4, 1.0, 0.8]),
        ]),
        seq(vec![
            equal("agent/model_speaking", 1.0),
            publish(&colors, &[number(0.2), number(1.0), number(0.6), glow]),
        ]),
        seq(vec![
            equal("agent/awaiting_response", 1.0),
            publish(
                &colors,
                &[number(0.8), number(0.5), number(1.0), thinking_pulse],
            ),
        ]),
        set(&colors, &[0.9, 0.7, 0.4, 0.3]),
    ]);
    let root = par(vec![
        safety,
        conversation,
        attention,
        base,
        led,
        pulse(
            "face/blink",
            "gap_min: 2.2, gap_max: 5.2, low: 0.7, high: 0.95, width: 0.1, salt: 3",
        ),
    ]);
    let events = events();
    json!({"ticks":TICKS,"warmUp":WARM_UP,"dt":DT,"clock":"arora/time","keys":keys,"text":format!("{root}\n\n{}",g.graph),"phases":phases(&events),"events":events})
}
pub fn workload() -> Workload {
    Workload::parse(&json().to_string()).expect("valid conversation workload")
}

struct Script {
    seed: u32,
    writes: BTreeMap<(usize, String), Value>,
}
impl Script {
    fn random(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        f64::from(self.seed) / 4294967296.0
    }
    fn span(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.random()
    }
    fn write(&mut self, s: f64, name: &str, value: impl Into<Value>) {
        let tick = (s / DT).round_ties_even().clamp(0.0, (TICKS - 1) as f64) as usize;
        let value = value.into();
        let value = if let Some(n) = value.as_f64() {
            json!((n * 10000.0).round_ties_even() / 10000.0)
        } else {
            value
        };
        self.writes.insert((tick, name.into()), value);
    }
}
fn events() -> Vec<Value> {
    let mut e = Script {
        seed: 20261022,
        writes: BTreeMap::new(),
    };
    let mut s = 1.0;
    let mut turn = 0;
    while s < 86.0 {
        if (38.0..47.0).contains(&s) {
            s = 48.0;
            continue;
        }
        let gap = e.span(2.0, 7.0);
        if e.random() < 0.35 {
            let start = s + e.span(0.2, gap * 0.4);
            let length = e.span(1.5, 3.5);
            e.write(start, "pointer/active", 1.0);
            let mut x = start;
            while x < start + length {
                e.write(x, "pointer/x", 0.5 + 0.35 * (x * 2.1).sin());
                e.write(x, "pointer/y", 0.5 + 0.25 * (x * 1.3 + 1.0).sin());
                x += DT;
            }
            e.write(start + length, "pointer/active", 0.0);
        }
        s += gap;
        let length = e.span(1.8, 5.5);
        e.write(s, "agent/user_speaking", 1.0);
        let mut x = s;
        while x < s + length {
            let v = 0.25 + 0.6 * (x * 9.0).sin().abs() * e.span(0.6, 1.0);
            e.write(x, "audio/user_energy", v);
            x += 2.0 * DT;
        }
        s += length;
        e.write(s, "agent/user_speaking", 0.0);
        e.write(s, "audio/user_energy", 0.0);
        e.write(s, "agent/awaiting_response", 1.0);
        s += e.span(0.5, 2.2);
        let length = e.span(2.5, 8.0);
        e.write(s, "agent/awaiting_response", 0.0);
        e.write(s, "agent/model_speaking", 1.0);
        e.write(
            s,
            "agent/response_text",
            [
                "Hello, how can I help?",
                "I can look over there.",
                "Let me think about that.",
                "It is good to see you.",
            ][turn % 4],
        );
        let interrupt = if e.random() < 0.15 {
            Some(s + e.span(1.0, length - 0.5))
        } else {
            None
        };
        let end = interrupt.map_or(s + length, |i| i + 0.3);
        // Advance the speech-cadence random draws without scripted viseme writes.
        let mut x = s;
        while x < end {
            e.random();
            x += e.span(0.045, 0.32);
        }
        let mut x = s;
        while x < end {
            let v = 0.2 + 0.7 * (x * 7.0).sin().abs() * e.span(0.5, 1.0);
            e.write(x, "audio/model_energy", v);
            x += 2.0 * DT;
        }
        if e.random() < 0.45 {
            let when = s + e.span(0.5, 2.0f64.min(length - 0.5));
            let tid = 1 + (e.random() * 6.0) as usize;
            if when < end {
                if tid == 1 {
                    for (name, lo, hi) in [
                        ("agent/tool/x", -0.6, 0.6),
                        ("agent/tool/y", -0.3, 0.4),
                        ("agent/tool/hold", 0.8, 2.0),
                    ] {
                        let v = e.span(lo, hi);
                        e.write(when, name, v);
                    }
                }
                if tid == 2 {
                    let which = (e.random() * 6.0) as usize;
                    for (i, emotion) in EMOTIONS.iter().enumerate() {
                        let v = if i == which { e.span(0.5, 1.0) } else { 0.0 };
                        e.write(when, &format!("agent/tool/emotion/{emotion}"), v);
                    }
                    let v = e.span(1.5, 3.0);
                    e.write(when, "agent/tool/length", v);
                }
                e.write(when, "agent/tool", tid as f64);
            }
        }
        if let Some(interrupt) = interrupt {
            e.write(interrupt, "agent/user_speaking", 1.0);
            let mut x = interrupt;
            while x < interrupt + e.span(1.0, 2.0) {
                e.write(x, "audio/user_energy", 0.3 + 0.5 * (x * 9.0).sin().abs());
                x += 2.0 * DT;
            }
            e.write(x, "agent/user_speaking", 0.0);
            e.write(x, "audio/user_energy", 0.0);
        }
        e.write(end, "agent/model_speaking", 0.0);
        e.write(end, "audio/model_energy", 0.0);
        s = end + 0.2;
        turn += 1;
    }
    e.write(38.0, "perception/present", 0.0);
    e.write(47.0, "perception/present", 1.0);
    let (mut az, mut el, mut x) = (10.0, 5.0, 0.0);
    while x < 90.0 {
        if !(38.0..47.0).contains(&x) {
            az = (az + e.span(-3.0, 3.0)).clamp(-50.0, 50.0);
            el = (el + e.span(-1.5, 1.5)).clamp(-20.0, 20.0);
            e.write(x, "perception/speaker_azimuth", az);
            e.write(x, "perception/speaker_elevation", el);
            let size = 0.08 + 0.1 * (0.5 + 0.5 * (x * 0.11).sin()) + e.span(-0.005, 0.005);
            e.write(x, "perception/size", size);
        }
        x += 4.0 * DT;
    }
    for second in 0..90 {
        let battery = 0.24 - 0.12 * second as f64 / 90.0;
        e.write(second as f64, "power/battery", battery);
        if battery < 0.15 {
            e.write(
                second as f64,
                "perception/dock_distance",
                (2.5 - 0.08 * (second as f64 - 67.0)).max(0.0),
            );
        }
    }
    e.write(62.0, "safety/estop", 1.0);
    e.write(63.5, "safety/estop", 0.0);
    e.writes
        .into_iter()
        .map(|((tick, key), value)| json!([tick, key, value]))
        .collect()
}
fn phases(events: &[Value]) -> Value {
    let mut flags = [false; 3];
    let mut current = None;
    let mut runs = vec![];
    let mut at = 0;
    while at < events.len() {
        let tick = events[at][0].as_u64().unwrap();
        let mut changed = false;
        while at < events.len() && events[at][0].as_u64() == Some(tick) {
            if let Some(i) = [
                "agent/user_speaking",
                "agent/awaiting_response",
                "agent/model_speaking",
            ]
            .iter()
            .position(|k| Some(*k) == events[at][1].as_str())
            {
                flags[i] = events[at][2].as_f64().unwrap() != 0.0;
                changed = true;
            }
            at += 1;
        }
        if changed {
            let phase = if flags[0] {
                1
            } else if flags[2] {
                3
            } else if flags[1] {
                2
            } else {
                0
            };
            if current != Some(phase) {
                runs.push(json!([tick, phase]));
                current = Some(phase);
            }
        }
    }
    if runs.first().is_none_or(|r| r[0] != 0) {
        runs.insert(0, json!([0, 0]));
    }
    json!({"names":["idle","listening","thinking","speaking"],"runs":runs})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{clock::Monotonic, forms::FORMS, measure};
    use arora_types::value::Value as AroraValue;

    /// Frames start on branch entry, survive another writer between frames,
    /// restart after interruption, and finish with the final pose before clearing.
    #[test]
    fn gestures_write_due_frames_and_restart_on_reentry() {
        for (name, frame_ticks, poses) in [
            (
                "cue",
                vec![0, 5, 21],
                vec![
                    vec![0.9, -0.35, -0.25],
                    vec![0.0, -0.35, -0.25],
                    vec![0.0; 3],
                ],
            ),
            (
                "nod",
                vec![0, 15, 30, 45],
                vec![vec![-0.3], vec![0.0], vec![-0.3], vec![0.0]],
            ),
            (
                "shake",
                vec![0, 12, 24, 36, 48],
                vec![vec![0.3], vec![-0.3], vec![0.3], vec![-0.3], vec![0.0]],
            ),
            (
                "wave",
                vec![0, 18, 36, 54, 84],
                vec![
                    vec![1.3, 0.8],
                    vec![1.3, 1.4],
                    vec![1.3, 0.8],
                    vec![1.3, 1.4],
                    vec![0.0; 2],
                ],
            ),
            ("point", vec![0, 90], vec![vec![0.9, 0.1], vec![0.0; 2]]),
            (
                "greet",
                vec![0, 30, 60, 90],
                vec![
                    vec![1.3, 0.8, 1.0],
                    vec![1.3, 1.4, 1.0],
                    vec![1.3, 0.8, 1.0],
                    vec![0.0; 3],
                ],
            ),
        ] {
            let outputs: Vec<_> = (0..poses[0].len()).map(|i| format!("out/{i}")).collect();
            let mut keys = vec![
                json!({"name":"clock","initial":0.0}),
                json!({"name":"gate","initial":0.0}),
                json!({"name":"done","initial":0.0}),
            ];
            keys.extend(outputs.iter().map(|k| json!({"name":k,"initial":0.0})));
            let last = *frame_ticks.last().unwrap();
            let fs: Vec<_> = frame_ticks
                .iter()
                .zip(&poses)
                .map(|(t, v)| (*t as f64 * DT, v.clone()))
                .collect();
            let text = fallback(vec![
                seq(vec![
                    equal("gate", 1.0),
                    frames(name, &outputs, &fs),
                    set(&["done".into()], &[1.0]),
                ]),
                "succeed".into(),
            ])
            .replace("arora/time", "clock");
            let raw = json!({"ticks":190,"warmUp":0,"dt":DT,"clock":"clock","keys":keys,"text":text,
                "events":[[12,"gate",1.0],[15,"out/0",0.77],[32,"gate",0.0],[70,"gate",1.0]],
                "phases":{"names":["gesture"],"runs":[]}});
            let workload = Workload::parse(&raw.to_string()).unwrap();
            let mut pose = vec![0.0; outputs.len()];
            let mut done = 0.0;
            let mut previous = None;
            let expected: Vec<_> = (0..190)
                .map(|tick| {
                    if tick == 15 {
                        pose[0] = 0.77;
                    }
                    if (12..32).contains(&tick) || tick >= 70 {
                        let start = if tick < 32 { 12 } else { 70 };
                        let frame = frame_ticks
                            .iter()
                            .rposition(|t| tick - start >= *t)
                            .unwrap();
                        if previous != Some((start, frame)) {
                            pose = poses[frame].clone();
                            previous = Some((start, frame));
                        }
                        if tick - start >= last {
                            done = 1.0;
                        }
                    }
                    let gate = if (12..32).contains(&tick) || tick >= 70 {
                        1.0
                    } else {
                        0.0
                    };
                    let entries = [
                        ("clock", (tick + 1) as f64 * DT),
                        ("done", done),
                        ("gate", gate),
                    ]
                    .into_iter()
                    .chain(outputs.iter().map(String::as_str).zip(pose.iter().copied()));
                    measure::values_hash(
                        &entries
                            .flat_map(|(name, value)| {
                                [
                                    Some(AroraValue::String(name.into())),
                                    Some(AroraValue::F64(value)),
                                ]
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            for form in FORMS {
                let replay =
                    measure::replay(&workload, *form, true, &Monotonic::default()).unwrap();
                assert_eq!(replay.written, expected, "{name}/{}", form.name());
            }
        }
    }

    /// Pin the seeded replay's public inputs, interruption windows and phase labels.
    #[test]
    fn script_has_sentences_interruptions_and_neutral_keys() {
        let raw = json();
        let events = raw["events"].as_array().unwrap();
        assert_eq!(events.len(), 6539);
        assert_eq!(raw["phases"]["runs"].as_array().unwrap().len(), 27);
        let contains = |tick, key, value| events.contains(&json!([tick, key, value]));
        assert!(contains(313, "agent/user_speaking", json!(1.0)));
        assert!(contains(
            667,
            "agent/response_text",
            json!("Hello, how can I help?")
        ));
        assert!(contains(1484, "agent/user_speaking", json!(1.0)));
        assert!(contains(1502, "agent/model_speaking", json!(0.0)));
        assert!(contains(2280, "perception/present", json!(0.0)));
        assert!(contains(2820, "perception/present", json!(1.0)));
        assert!(contains(3720, "safety/estop", json!(1.0)));
        assert!(contains(3810, "safety/estop", json!(0.0)));
        assert_eq!(
            events.iter().filter(|e| e[1] == "power/battery").count(),
            90
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e[1] == "agent/response_text")
                .count(),
            6
        );
        assert!(!events
            .iter()
            .any(|e| e[1].as_str().unwrap().starts_with("speech/")));
        for key in raw["keys"].as_array().unwrap() {
            let name = key["name"].as_str().unwrap();
            assert!(
                [
                    "arora/",
                    "perception/",
                    "agent/",
                    "audio/",
                    "power/",
                    "safety/",
                    "pointer/",
                    "body/",
                    "face/",
                    "speech/"
                ]
                .iter()
                .any(|p| name.starts_with(p)),
                "{name}"
            );
        }
        let workload = workload();
        assert_eq!(workload.phases[1484], 1);
        assert_eq!(workload.phases[1502], 1);
        assert_eq!(workload.phases[1558], 0);
    }
}
