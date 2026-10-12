//! Probes a NAOqi robot (or `naoqi-sim`) through the HAL: describes it, streams a few seconds
//! of updates, then commands a joint, lights the face LEDs and says a sentence.
//!
//! Usage: `cargo run -p arora-hal-naoqi --example naoqi_probe -- tcp://127.0.0.1:9559`

use std::time::Duration;

use arora_behavior::Status;
use arora_hal::Hal;
use arora_hal_naoqi::say::{self, naoqi_say};
use arora_hal_naoqi::{NaoqiHal, NaoqiRobotConfig};
use arora_types::data::{Key, StateChange};
use arora_types::value::Value;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tcp://127.0.0.1:9559".to_string());
    let hal = NaoqiHal::new(NaoqiRobotConfig::with_url(url)).await?;
    println!("description: {:?}", hal.describe().await);

    let mut updates = hal.updates();
    let mut received = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while let Ok(Some(change)) = tokio::time::timeout_at(deadline, updates.next()).await {
        received += change.len();
        for key in [
            "head_yaw.position",
            "battery.charge",
            "imu.angle.x",
            "sonar.left.distance",
        ] {
            if let Some(value) = change.set.get(&Key::from(key)) {
                println!("{key} = {value:?}");
            }
        }
    }
    println!("{received} key updates in 2 seconds");

    let mut change = StateChange::set("head_yaw.target_position", Value::F64(0.4));
    change.set.insert(
        Key::from("led.FaceLeds.color"),
        Some(Value::U32(0x0000_ff00)),
    );
    hal.write(change).await?;

    // Speech is a behavior leaf: tick `say` as a tree would, until the sentence is said.
    say::install(hal.voice());
    let mut viseme = String::new();
    let mut status = Status::Running;
    while status == Status::Running {
        status = naoqi_say::say("Hello from Arora".to_string(), String::new(), &mut viseme);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    println!("say: {status:?}");
    let read = hal.read(&[Key::from("head_yaw.position")]).await?;
    println!("head_yaw.position after the command: {:?}", read[0]);
    Ok(())
}
