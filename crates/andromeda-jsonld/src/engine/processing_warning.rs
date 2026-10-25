//! Logging the warnings JSON-LD context processing raises.
//!
//! Context processing reports a keyword-like term or value and a malformed IRI
//! as a warning, not a failure (JSON-LD 1.1 context processing algorithm), so
//! the context still processes. The broker logs each one so an operator can see
//! a context that processes with terms the algorithm ignores.

use jsonld::context_processing::Warning;
use jsonld_core::warning::Handler;
use tracing::warn;

/// Warning handler that logs every context-processing warning at `warn` level.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LoggedWarnings;

impl<N> Handler<N, Warning> for LoggedWarnings {
    fn handle(&mut self, _vocabulary: &N, warning: Warning) {
        warn!(%warning, "JSON-LD context processing raised a warning");
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::processing_warning::LoggedWarnings;
    use jsonld::context_processing::Warning;
    use jsonld_core::warning::Handler;

    #[test]
    fn logged_warnings_accepts_every_context_processing_warning() {
        let mut handler = LoggedWarnings;
        handler.handle(&(), Warning::KeywordLikeTerm("@foo".to_owned()));
        handler.handle(&(), Warning::KeywordLikeValue("@bar".to_owned()));
        handler.handle(&(), Warning::MalformedIri("not an iri".to_owned()));
    }
}
