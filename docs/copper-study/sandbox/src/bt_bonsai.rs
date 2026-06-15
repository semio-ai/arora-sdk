//! A real behavior tree built with bonsai-bt 0.12, ticked over time, plus a
//! serde round-trip proving "trees as data". This is the closest off-the-shelf
//! Rust BT to pair with Copper (Copper itself has no BT). Contrast with Arora:
//! here the *leaf action set* (`Action` enum) is fixed at compile time; in Arora
//! a leaf is a call into a dynamically loaded module function.
//! Run: `cargo run --release --bin bt_bonsai`
use bonsai_bt::{Action, Event, Failure, If, Select, Sequence, Success, UpdateArgs, Wait, BT};
use std::collections::HashMap;

// The action/condition set is an enum known at COMPILE TIME.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Act {
    BatteryAbove(i64), // condition: battery% > N  (i64 so the tree is serde-friendly)
    MoveTo(i64),       // move joint to a target (centi-radians)
    Wave,              // play a gesture
    GoCharge,          // fallback behaviour
}

type Board = HashMap<String, i64>;

fn run_tree(label: &str, behavior: bonsai_bt::Behavior<Act>, battery: i64) -> (String, i64) {
    let mut bb: Board = HashMap::new();
    bb.insert("battery".into(), battery);
    bb.insert("position".into(), 0);
    let mut bt = BT::new(behavior, bb);
    let mut last = String::from("none");
    // tick a few times, advancing virtual time so Wait nodes elapse
    for _ in 0..8 {
        let e: Event = UpdateArgs { dt: 1.0 }.into();
        let _ = bt.tick(&e, &mut |args, board: &mut Board| {
            match args.action {
                Act::BatteryAbove(th) => {
                    let ok = *board.get("battery").unwrap() > *th;
                    last = format!("BatteryAbove({th})={ok}");
                    (if ok { Success } else { Failure }, args.dt)
                }
                Act::MoveTo(p) => { board.insert("position".into(), *p); last = format!("MoveTo({p})"); (Success, args.dt) }
                Act::Wave => { last = "Wave".into(); (Success, args.dt) }
                Act::GoCharge => { board.insert("position".into(), -1); last = "GoCharge".into(); (Success, args.dt) }
            }
        });
    }
    let pos = *bt.blackboard_mut().get("position").unwrap();
    println!("[{label}] battery={battery} last_action={last} final_position={pos}");
    (last, pos)
}

fn make_tree() -> bonsai_bt::Behavior<Act> {
    // If battery healthy: approach + wave. Otherwise: go charge.
    Select(vec![
        Sequence(vec![
            If(
                Box::new(Action(Act::BatteryAbove(20))),
                Box::new(Sequence(vec![Wait(1.0), Action(Act::MoveTo(150)), Action(Act::Wave)])),
                Box::new(Action(Act::GoCharge)),
            ),
        ]),
        Action(Act::GoCharge),
    ])
}

fn main() {
    // 1. Tick the in-memory tree with two different battery levels.
    let (_a1, p_full) = run_tree("battery-ok", make_tree(), 80);
    let (_a2, p_low) = run_tree("battery-low", make_tree(), 5);

    // 2. Trees as data: serialise the Behavior to JSON, reload it, tick again.
    let json = serde_json::to_string(&make_tree()).unwrap();
    let reloaded: bonsai_bt::Behavior<Act> = serde_json::from_str(&json).unwrap();
    let (_a3, p_reloaded) = run_tree("reloaded-from-json", reloaded, 80);

    println!("RESULT json_len={} pos_full={} pos_low={} pos_reloaded={} roundtrip_ok={}",
        json.len(), p_full, p_low, p_reloaded, p_full == p_reloaded);
}
