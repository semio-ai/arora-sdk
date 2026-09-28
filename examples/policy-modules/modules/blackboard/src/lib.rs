//! Leaves over the store's strings, for trees that switch on a request.
//!
//! A tree cannot compare a key to a literal by itself: its control nodes
//! only route on their children's status. `equals` turns a comparison into a
//! status, so a `Sequence` headed by it runs its branch only while a key
//! holds a value; `assign` writes a value, so a branch can hand the request
//! back once its work is done. With the two, a request written from outside
//! the tree — an editor's live data, a script, another device — selects a
//! branch, which is how a tree switches behaviors on something other than a
//! sensor.

use arora_behavior::Status;

#[arora_module::module(
    id = "83c5cdba-f0ec-4ce1-adad-ad97c287f274",
    name = "blackboard",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "Test and assign the store's strings from a behavior tree",
    executable_mime = "application/wasm"
)]
pub mod blackboard {
    use super::*;

    /// Succeeds when `value` is `expected`, fails otherwise — a condition
    /// leaf.
    #[export(id = "fd5ffb52-4b01-4d8e-a788-d7167efcf5f7")]
    pub fn equals(
        #[param(id = "2b91dcc1-e27b-46bd-b3e2-8045c2e9b11b")] value: String,
        #[param(id = "30ce0392-f016-415e-a22d-7f0fe30e185b")] expected: String,
    ) -> Status {
        if value == expected {
            Status::Success
        } else {
            Status::Failure
        }
    }

    /// Writes `value` into `target` and succeeds.
    #[export(id = "3655eb4b-c673-4d69-89af-3dc10a090374")]
    pub fn assign(
        #[param(id = "2b91dcc1-e27b-46bd-b3e2-8045c2e9b11b")] value: String,
        #[param(id = "192456d1-3695-4c2d-b3f4-a611b7a3346e")] target: &mut String,
    ) -> Status {
        target.clone_from(&value);
        Status::Success
    }
}

#[cfg(test)]
mod tests {
    use super::blackboard::*;
    use super::*;

    #[test]
    fn equals_is_a_condition() {
        assert_eq!(equals("kick".into(), "kick".into()), Status::Success);
        assert_eq!(equals("walk".into(), "kick".into()), Status::Failure);
    }

    #[test]
    fn assign_writes_the_value() {
        let mut target = "kick".to_string();
        assert_eq!(assign("walk".into(), &mut target), Status::Success);
        assert_eq!(target, "walk");
    }
}
