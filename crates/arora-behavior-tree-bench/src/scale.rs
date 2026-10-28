//! Deterministic scale scenarios written in the behavior text language.
use crate::model::Workload;
use serde_json::{json, Value};
use std::collections::HashSet;

const DT: f64 = 1.0 / 30.0;
pub const SIZES: &[usize] = &[10, 30, 100, 300, 1000];

#[derive(Clone, Copy, PartialEq)]
pub enum Scenario {
    Settled,
    OneInput,
    AllInputs,
    Clock,
    Branches,
    BranchesSwitching,
}
impl Scenario {
    pub const ALL: [Scenario; 6] = [
        Self::Settled,
        Self::OneInput,
        Self::AllInputs,
        Self::Clock,
        Self::Branches,
        Self::BranchesSwitching,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Settled => "settled",
            Self::OneInput => "one-input",
            Self::AllInputs => "all-inputs",
            Self::Clock => "clock",
            Self::Branches => "branches",
            Self::BranchesSwitching => "branches-switching",
        }
    }
    pub fn workload(self, n: usize, warm_up: usize, ticks: usize) -> Workload {
        assert!(n > 0 && ticks > 0);
        let branches = matches!(self, Self::Branches | Self::BranchesSwitching);
        let (parts, size) = if branches { (8, n.div_ceil(8)) } else { (1, n) };
        let k = 2.max(size / 10);
        let mut random = Random(332_396);
        let mut keys = Vec::new();
        let mut inputs = Vec::new();
        let mut on = Vec::new();
        let mut tree = if branches {
            "fallback\n".to_string()
        } else {
            "running\n".to_string()
        };
        let mut text = String::new();
        for part in 0..parts {
            let prefix = if branches {
                format!("branch/{part}/")
            } else {
                String::new()
            };
            let names: Vec<_> = (0..k).map(|i| format!("{prefix}input/{i}")).collect();
            for name in &names {
                keys.push(json!({"name":name,"initial":if branches {0.25} else {0.0}}));
            }
            inputs.extend(names.iter().cloned());
            let outputs: Vec<_> = if branches {
                Vec::new()
            } else {
                (0..k)
                    .map(|i| {
                        let name = format!("output/{i}");
                        keys.push(json!({"name":name,"initial":null}));
                        name
                    })
                    .collect()
            };
            let first = part * size;
            let mut read = HashSet::new();
            let mut expressions = Vec::new();
            for i in 0..size {
                let expression = if branches {
                    let p = random.next();
                    let x = if i < k {
                        format!("{{{}}}", names[i])
                    } else if random.next() < 0.8 {
                        let back = (random.next() * i.min(32) as f64).floor() as usize;
                        let source = first + i - 1 - back;
                        read.insert(source);
                        format!("@n{source}")
                    } else {
                        format!("{{{}}}", names[(random.next() * k as f64).floor() as usize])
                    };
                    if p < 0.30 {
                        format!("add(operand_0: {x}, operand_1: 0.02f32)")
                    } else if p < 0.60 {
                        format!(
                            "multiply(operand_0: {x}, operand_1: {}f32)",
                            0.5 + random.next() * 0.3
                        )
                    } else if p < 0.75 {
                        format!("clamp(in: {x}, min: -1f32, max: 1f32)")
                    } else if p < 0.95 {
                        format!("remap(in: {x}, in_min: -2f32, in_max: 2f32, out_min: -0.5f32, out_max: 0.5f32)")
                    } else if self == Self::BranchesSwitching {
                        format!("add(operand_0: {x})")
                    } else {
                        format!("damp(in: {x}, param.half_life: 0.06f32, dt: 0.033333335f32, state: ())")
                    }
                } else {
                    let x = if i < k {
                        format!("{{{}}}", names[i])
                    } else {
                        format!("@n{}", i - k)
                    };
                    if self == Self::Clock && i < k {
                        "oscillator(frequency: 0.7f32, t: {arora/time})".into()
                    } else {
                        match (i / k) % 4 {
                        0 => format!("add(operand_0: {x}, operand_1: 1.1f32)"),
                        1 => format!("multiply(operand_0: {x}, operand_1: 1.1f32)"),
                        2 => format!("clamp(in: {x}, min: 0f32, max: 2.1f32)"),
                        _ => format!("remap(in: {x}, in_min: 1.1f32, in_max: 3.1f32, out_min: 0f32, out_max: 1f32)"),
                    }
                    }
                };
                expressions.push(expression);
            }
            let mut published: Vec<_> = if branches {
                (first..first + size)
                    .filter(|c| !read.contains(c))
                    .collect()
            } else {
                Vec::new()
            };
            for c in first..first + size {
                if published.len() >= size.div_ceil(10) {
                    break;
                }
                if !published.contains(&c) {
                    published.push(c);
                }
            }
            if branches {
                let name = format!("{prefix}on");
                tree.push_str(&format!(
                    "  sequence\n    equal(a: {{{name}}}, b: true)\n    parallel\n"
                ));
                for &c in &published {
                    let name = format!("{prefix}output/b{part}_n{}", c - first);
                    keys.push(json!({"name":name,"initial":null}));
                    tree.push_str(&format!("      {{{name}}} = add(operand_0: @n{c})\n"));
                }
                tree.push_str("      running\n");
                keys.push(json!({"name":name,"initial":part == 0}));
                on.push(name);
            }
            for (i, mut expression) in expressions.into_iter().enumerate() {
                let c = first + i;
                let target = if !branches && i + k >= size {
                    format!("{{{}}} = ", outputs[i % k])
                } else {
                    String::new()
                };
                expression.pop();
                text.push_str(&format!("n{c}: {target}{expression}, node: \"n{c}\")\n"));
            }
        }
        keys.push(json!({"name":"arora/time","initial":0.0}));
        let mut events = Vec::new();
        for step in 0..warm_up + ticks {
            match self {
                Self::Settled | Self::Clock => {}
                Self::OneInput => events.push(json!([step, inputs[0], step % 17])),
                Self::AllInputs => {
                    for name in &inputs {
                        events.push(json!([step, name, step % 17]));
                    }
                }
                Self::Branches | Self::BranchesSwitching => {
                    for (i, name) in inputs.iter().enumerate() {
                        events.push(json!([step, name, ((step + i) % 17) as f64 / 8.0 - 1.0]));
                    }
                    let taken = if self == Self::Branches {
                        0
                    } else {
                        step / 50 % 8
                    };
                    for (part, name) in on.iter().enumerate() {
                        events.push(json!([step, name, part == taken]));
                    }
                }
            }
        }
        let raw: Value = json!({"keys":keys,"text":format!("{tree}\n{text}"),"clock":"arora/time","dt":DT,"ticks":warm_up+ticks,"warmUp":warm_up,
            "events":events,"phases":{"names":[self.name()],"runs":[]}});
        Workload::parse(&raw.to_string()).expect("valid scale workload")
    }
}

/// A 32-bit linear congruential generator, scaled to [0, 1).
struct Random(u32);
impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.0) / 4_294_967_296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_behavior_tree::schema::Expression;
    use arora_dataflow_nodes::{
        functions::{function_id, node_param},
        Kind,
    };
    use arora_types::value::Value;

    #[test]
    fn chain_operations_follow_depth_for_every_default_size() {
        for &n in SIZES {
            let workload = Scenario::Settled.workload(n, 0, 1);
            let k = 2.max(n / 10);
            let mut calls = 0;
            for node in &workload.nodes.0 {
                let Some(Expression::Value(Value::String(name))) =
                    node.arguments.get(&node_param())
                else {
                    continue;
                };
                let i: usize = name.strip_prefix('n').unwrap().parse().unwrap();
                assert_eq!(
                    node.function,
                    function_id(
                        &[Kind::Add, Kind::Multiply, Kind::Clamp, Kind::Remap][(i / k) % 4]
                    )
                );
                calls += 1;
            }
            assert_eq!(calls, n);
        }
    }
}
