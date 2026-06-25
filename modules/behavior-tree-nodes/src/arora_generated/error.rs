use derive_more::Display;
#[derive(Display, Debug)]
pub struct DeserializationError {
    #[display("deserialization error: {}", message)]
    pub message: String,
}
impl std::error::Error for DeserializationError {}
