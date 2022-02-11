use derive_more::Display;

#[derive(Display, Debug)]
pub struct DeserializationError {}

impl std::error::Error for DeserializationError {}
