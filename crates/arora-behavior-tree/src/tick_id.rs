use arora::call::CallableId;
use uuid::Uuid;

/// An alternative to CallableId that refers to callables returning a Status.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct TickId {
  pub callable_id: u64,
}

impl From<&TickId> for CallableId {
  fn from(val: &TickId) -> Self {
    CallableId {
      id: val.callable_id,
    }
  }
}

impl From<TickId> for CallableId {
  fn from(val: TickId) -> Self {
    CallableId {
      id: val.callable_id,
    }
  }
}

impl From<CallableId> for TickId {
  fn from(callable_id: CallableId) -> Self {
    Self {
      callable_id: callable_id.id,
    }
  }
}

lazy_static::lazy_static! {
  pub static ref TICK_ID_TYPE_ID: Uuid = Uuid::parse_str("6f49e650-84ca-4899-a9bd-1f3bf17fab51").unwrap();
  pub static ref TICK_ID_ID_FIELD_ID: Uuid = Uuid::parse_str("237992d2-17d1-459f-bca1-7185fa6a69d7").unwrap();
}