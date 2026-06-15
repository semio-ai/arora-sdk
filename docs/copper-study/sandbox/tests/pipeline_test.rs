//! `cargo test` proof that a Copper static DAG compiles from RON and runs.
//! Mirrors src/minimal_pipeline.rs but asserts the result instead of printing.
use cu29::prelude::*;
use std::sync::atomic::{AtomicI32, Ordering};

static LAST: AtomicI32 = AtomicI32::new(0);

#[derive(Default, Reflect)] pub struct TCounter { n: i32 }
impl Freezable for TCounter {}
impl CuSrcTask for TCounter {
    type Output<'m> = output_msg!(i32);
    type Resources<'r> = ();
    fn new(_c: Option<&ComponentConfig>, _r: Self::Resources<'_>) -> CuResult<Self> { Ok(Self { n: 0 }) }
    fn process(&mut self, _ctx: &CuContext, o: &mut Self::Output<'_>) -> CuResult<()> { self.n += 1; o.set_payload(self.n); Ok(()) }
}
#[derive(Default, Reflect)] pub struct TDoubler;
impl Freezable for TDoubler {}
impl CuTask for TDoubler {
    type Input<'m> = input_msg!(i32);
    type Output<'m> = output_msg!(i32);
    type Resources<'r> = ();
    fn new(_c: Option<&ComponentConfig>, _r: Self::Resources<'_>) -> CuResult<Self> { Ok(Self) }
    fn process(&mut self, _ctx: &CuContext, i: &Self::Input<'_>, o: &mut Self::Output<'_>) -> CuResult<()> { if let Some(v)=i.payload(){o.set_payload(*v*2);} Ok(()) }
}
#[derive(Default, Reflect)] pub struct TCollector;
impl Freezable for TCollector {}
impl CuSinkTask for TCollector {
    type Input<'m> = input_msg!(i32);
    type Resources<'r> = ();
    fn new(_c: Option<&ComponentConfig>, _r: Self::Resources<'_>) -> CuResult<Self> { Ok(Self) }
    fn process(&mut self, _ctx: &CuContext, i: &Self::Input<'_>) -> CuResult<()> { if let Some(v)=i.payload(){ LAST.store(*v, Ordering::SeqCst);} Ok(()) }
}

#[copper_runtime(config = "copperconfig_test.ron")]
struct TestApp {}

#[test]
fn copper_static_dag_runs_from_ron() {
    let tmp = std::env::temp_dir().join(format!("copper_test_{}.copper", std::process::id()));
    let mut app = TestApp::builder().with_log_path(&tmp, Some(64*1024*1024)).unwrap().build().unwrap();
    app.start_all_tasks().unwrap();
    for _ in 0..5 { app.run_one_iteration().unwrap(); }
    app.stop_all_tasks().unwrap();
    // src emits 1..=5, doubler doubles the last -> 10
    assert_eq!(LAST.load(Ordering::SeqCst), 10);
}
