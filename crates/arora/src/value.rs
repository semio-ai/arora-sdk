use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Value {
  ty: u128,
  data: Box<[u8]>,
}