mod error;
mod interning;
mod trie;

pub use error::InternError;
pub use interning::{Intern, InternRef, Interner};
