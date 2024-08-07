use thiserror::Error;

/// Represents the only error that an interner can encounter while
/// interning. A thread can't wait on itself to drop an InternRef
/// of course.
#[derive(Debug, Error)]
pub enum InternError {
  #[error("Cannot intern while this thread holds an InternRef")]
  OutstandingLocalRef(&'static std::panic::Location<'static>),
}
