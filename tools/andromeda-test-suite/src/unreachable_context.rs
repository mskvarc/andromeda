//! An `@context` reference to a document nothing serves: a path under the stack's unreachable origin, which a broker
//! cannot download (clause 5.5.6).

use crate::{config::SuiteConfig, config_error::ConfigError};
use iri_rs::{IriBuf, IriRefBuf};
use ngsi_ld::model::{Context, ContextEntry};

/// Why the reference cannot be written.
#[derive(Debug, thiserror::Error)]
pub enum UnreachableContextError {
    /// The test configuration is missing or invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The path does not join the origin.
    #[error("the path does not join the unreachable origin")]
    Path(#[source] url::ParseError),
    /// The joined URL is not an IRI.
    #[error("the unreachable context URL {0} is not an IRI")]
    NotAnIri(url::Url),
}

/// The context entry naming `path` under the unreachable origin.
///
/// # Errors
///
/// Returns [`UnreachableContextError`] when the configuration is missing or the joined URL is not an IRI.
pub fn unreachable_entry(path: &str) -> Result<ContextEntry, UnreachableContextError> {
    let origin = SuiteConfig::get().map_err(Clone::clone)?.unreachable_origin();
    let url = origin.join(path).map_err(UnreachableContextError::Path)?;
    let iri = IriBuf::new(url.as_str().to_owned()).map_err(|_| UnreachableContextError::NotAnIri(url))?;
    Ok(ContextEntry::Reference(IriRefBuf::from(iri)))
}

/// The context naming `path` under the unreachable origin, as one reference.
///
/// # Errors
///
/// Returns [`UnreachableContextError`] when the configuration is missing or the joined URL is not an IRI.
pub fn unreachable_context(path: &str) -> Result<Context, UnreachableContextError> {
    Ok(Context::Single(unreachable_entry(path)?))
}

#[cfg(test)]
mod tests {
    use crate::unreachable_context::{unreachable_context, unreachable_entry};
    use googletest::prelude::*;
    use ngsi_ld::model::{Context, ContextEntry};

    #[gtest]
    fn the_context_is_the_entry_alone_or_both_fail_on_the_configuration() -> Result<()> {
        match (unreachable_context("missing.jsonld"), unreachable_entry("missing.jsonld")) {
            (Ok(Context::Single(ContextEntry::Reference(context))), Ok(ContextEntry::Reference(entry))) => verify_that!(context, eq(&entry)),
            (Err(_), Err(_)) => Ok(()),
            (context, entry) => fail!("the context {context:?} and the entry {entry:?} disagree"),
        }
    }
}
