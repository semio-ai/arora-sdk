// Let us pretend this is an auto-generated header.
//====================================================================================
// Provides the interface of the module, and forwards calls to specialized functions.
mod arora_generated;
// Provides symbols imported from other modules.
use arora_generated::status::Status;
//====================================================================================
// Put the implementation below.

fn ping() { }
fn succeed() -> Status { Status::Success }

// Tests
//====================================================================================
#[cfg(test)]
pub mod tests {
  use super::*;
  use anyhow::Result;

  #[test]
  pub fn test_ping() -> Result<()> {
    ping();
    Ok(())
  }
}