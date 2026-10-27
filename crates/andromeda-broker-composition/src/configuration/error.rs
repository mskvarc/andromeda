//! Why the configuration could not be made ready for the broker.

use crate::configuration::section_admission::SectionAdmissionError;
use andromeda_configuration::{load::LoadError, redact::RenderError};
use thiserror::Error;

/// The configuration did not load, a domain refused a section value, or the
/// effective configuration could not be written out.
#[derive(Debug, Error)]
pub(crate) enum ConfigurationError {
    /// The configuration did not load.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// A domain refused a section value.
    #[error(transparent)]
    Admission(#[from] SectionAdmissionError),
    /// The effective configuration could not be written out.
    #[error(transparent)]
    Render(#[from] RenderError),
}
