use thiserror::Error;

#[derive(Debug, Error)]
pub enum InternError {
  #[error("Cannot intern while holding an InternRef")]
  OutstandingRef(&'static std::panic::Location<'static>),
}
