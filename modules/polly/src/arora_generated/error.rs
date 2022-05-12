use derive_more::Display;
#[derive(Display, Debug)]
pub struct DeserializationError {
  #[display(fmt = "deserialization error: {}", message)]
  pub message: String,
}
impl std::error::Error for DeserializationError {}
