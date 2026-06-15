//! Minimal REAL Copper pipeline: Counter -> Doubler -> Collector.
//! Verified against cu29 1.0.0-rc2. The DAG is declared in copperconfig.ron
//! and the #[copper_runtime] proc macro generates the runtime at compile time.
use cu29::prelude::*;
use std::sync::atomic::{AtomicI32, Ordering};

// Observe the sink's last value across the generated runtime boundary.
static LAST: AtomicI32 = AtomicI32::new(0);

#[derive(Default, Reflect)]
pub struct Counter { n: i32 }
impl Freezable for Counter {}
impl CuSrcTask for Counter {
    type Output<'m> = output_msg!(i32);
    type Resources<'r> = ();
    fn new(_config: Option<&ComponentConfig>, _res: Self::Resources<'_>) -> CuResult<Self> { Ok(Self { n: 0 }) }
    fn process(&mut self, _ctx: &CuContext, output: &mut Self::Output<'_>) -> CuResult<()> {
        self.n += 1;
        output.set_payload(self.n);
        Ok(())
    }
}

#[derive(Default, Reflect)]
pub struct Doubler;
impl Freezable for Doubler {}
impl CuTask for Doubler {
    type Input<'m> = input_msg!(i32);
    type Output<'m> = output_msg!(i32);
    type Resources<'r> = ();
    fn new(_config: Option<&ComponentConfig>, _res: Self::Resources<'_>) -> CuResult<Self> { Ok(Self) }
    fn process(&mut self, _ctx: &CuContext, input: &Self::Input<'_>, output: &mut Self::Output<'_>) -> CuResult<()> {
        if let Some(v) = input.payload() { output.set_payload(*v * 2); }
        Ok(())
    }
}

#[derive(Default, Reflect)]
pub struct Collector;
impl Freezable for Collector {}
impl CuSinkTask for Collector {
    type Input<'m> = input_msg!(i32);
    type Resources<'r> = ();
    fn new(_config: Option<&ComponentConfig>, _res: Self::Resources<'_>) -> CuResult<Self> { Ok(Self) }
    fn process(&mut self, _ctx: &CuContext, input: &Self::Input<'_>) -> CuResult<()> {
        if let Some(v) = input.payload() { LAST.store(*v, Ordering::SeqCst); }
        Ok(())
    }
}

#[copper_runtime(config = "copperconfig.ron")]
struct App {}

fn main() -> CuResult<()> {
    let tmp = std::env::temp_dir().join(format!("copper_minimal_{}.copper", std::process::id()));
    let mut app = App::builder().with_log_path(&tmp, Some(64 * 1024 * 1024))?.build()?;
    app.start_all_tasks()?;
    for _ in 0..5 { app.run_one_iteration()?; }
    app.stop_all_tasks()?;
    println!("RESULT last_collected={}", LAST.load(Ordering::SeqCst));
    Ok(())
}
