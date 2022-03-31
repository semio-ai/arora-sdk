use arora_vfs::{Directory, File};
use convert_case::{Case, Casing};
use quote::{__private::TokenStream, quote};

pub fn token_stream_to_file(file_name: String, tokens: &TokenStream) -> Directory {
  let mut output = Directory::new();
  output.insert(file_name, File::new(tokens.to_string()));
  output
}
