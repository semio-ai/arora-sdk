//! Measures the cost of one Copper runtime iteration over the 3-task DAG.
//! Each iteration runs the full topological chain (src -> dbl -> sink) passing
//! i32 payloads through pre-allocated CopperList slots (zero-copy, no serde).
use cu29::prelude::*;
use std::hint::black_box;
use std::time::Instant;

#[derive(Default, Reflect)]
pub struct Counter { n: i32 }
impl Freezable for Counter {}
impl CuSrcTask for Counter {
    type Output<'m> = output_msg!(i32);
    type Resources<'r> = ();
    fn new(_c: Option<&ComponentConfig>, _r: Self::Resources<'_>) -> CuResult<Self> { Ok(Self { n: 0 }) }
    fn process(&mut self, _ctx: &CuContext, output: &mut Self::Output<'_>) -> CuResult<()> {
        self.n = self.n.wrapping_add(1);
        output.set_payload(black_box(self.n));
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
    fn new(_c: Option<&ComponentConfig>, _r: Self::Resources<'_>) -> CuResult<Self> { Ok(Self) }
    fn process(&mut self, _ctx: &CuContext, input: &Self::Input<'_>, output: &mut Self::Output<'_>) -> CuResult<()> {
        if let Some(v) = input.payload() { output.set_payload(black_box(v.wrapping_mul(2))); }
        Ok(())
    }
}
#[derive(Default, Reflect)]
pub struct Collector;
impl Freezable for Collector {}
impl CuSinkTask for Collector {
    type Input<'m> = input_msg!(i32);
    type Resources<'r> = ();
    fn new(_c: Option<&ComponentConfig>, _r: Self::Resources<'_>) -> CuResult<Self> { Ok(Self) }
    fn process(&mut self, _ctx: &CuContext, input: &Self::Input<'_>) -> CuResult<()> {
        if let Some(v) = input.payload() { black_box(*v); }
        Ok(())
    }
}

#[copper_runtime(config = "copperconfig_chain.ron")]
struct App {}

fn main() -> CuResult<()> {
    let tmp = std::env::temp_dir().join(format!("copper_tput_{}.copper", std::process::id()));
    let mut app = App::builder().with_log_path(&tmp, Some(256 * 1024 * 1024))?.build()?;
    app.start_all_tasks()?;
    // warmup
    for _ in 0..10_000 { app.run_one_iteration()?; }
    let n = 200_000u64;
    let t = Instant::now();
    for _ in 0..n { app.run_one_iteration()?; }
    let elapsed = t.elapsed();
    app.stop_all_tasks()?;
    let per = elapsed.as_nanos() as f64 / n as f64;
    println!("RESULT copper_iter_ns={:.1} iters={} total_ms={:.1}", per, n, elapsed.as_secs_f64()*1e3);
    Ok(())
}
