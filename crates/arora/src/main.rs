//! The default `arora` binary: the device runner, headless (no HAL-attached
//! display — a head like Vizij embeds [`arora::run_with_hal`] instead).
//!
//! It reads its configuration from the environment (Firebase options, Zenoh
//! endpoints, identity file), loads/saves an encrypted refresh token locally,
//! connects to Semio Studio over Zenoh, and runs the arora runtime. See
//! [`arora::run_with_hal`] for the configuration env vars and the full run.
//!
//! A device-specific build is a thin downstream binary that depends on `arora`
//! plus its own HAL/bridge crates and calls [`arora::run_with_hal`] /
//! [`arora::run_with`] with those implementations — customization from the
//! outside, no feature flags inside `arora`.

fn main() -> anyhow::Result<()> {
    arora::run()
}
