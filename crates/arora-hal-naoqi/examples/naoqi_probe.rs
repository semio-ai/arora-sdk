//! Probes a NAOqi robot (or `naoqi-sim`) through the HAL: describes it, streams a few seconds
//! of updates, then commands a joint, says a sentence and lights the face LEDs.
//!
//! Usage: `cargo run -p arora-hal-naoqi --example naoqi_probe -- tcp://127.0.0.1:9559`

use std::time::Duration;

use arora_hal::Hal;
use arora_hal_naoqi::{keys, NaoqiHal, NaoqiRobotConfig};
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
    change
        .set
        .insert(Key::from(keys::TEXT), Some(Value::from("Hello from Arora")));
    change.set.insert(
        Key::from("led.FaceLeds.color"),
        Some(Value::U32(0x0000_ff00)),
    );
    hal.write(change).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let read = hal
        .read(&[Key::from("head_yaw.position"), Key::from(keys::TEXT)])
        .await?;
    println!("head_yaw.position after the command: {:?}", read[0]);
    println!("text after speaking (unset once said): {:?}", read[1]);
    Ok(())
}
