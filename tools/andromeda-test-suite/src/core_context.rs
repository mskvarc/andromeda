//! The NGSI-LD core `@context` the stack's brokers serve, as the `@context` a request body names (clause 5.5.7).

use crate::{config::SuiteConfig, config_error::ConfigError};
use iri_rs::{IriBuf, IriRefBuf};
use ngsi_ld::{
    http::headers::{Link, LinkBuildError},
    model::{Context, ContextEntry},
};
use url::Url;

/// Why the core `@context` cannot be named.
#[derive(Debug, thiserror::Error)]
pub enum CoreContextError {
    /// The suite configuration is missing or invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The configured URL is not an IRI.
    #[error("the core @context URL {0} is not an IRI")]
    NotAnIri(Url),
    /// The core `@context` URL cannot be carried in a `Link` header.
    #[error(transparent)]
    Link(#[from] LinkBuildError),
}

/// The configured core `@context` URL as an IRI.
fn core_iri() -> Result<IriBuf, CoreContextError> {
    let url = SuiteConfig::get().map_err(Clone::clone)?.core_context();
    // clone: the error names the URL the configuration keeps
    IriBuf::new(url.as_str().to_owned()).map_err(|_| CoreContextError::NotAnIri(url.clone()))
}

/// The core `@context` written as a string: `"@context": "<core>"`.
///
/// # Errors
///
/// Returns [`CoreContextError`] when the configuration is missing or its URL is not an IRI.
pub fn core_context() -> Result<Context, CoreContextError> {
    Ok(Context::from(core_iri()?))
}

/// The core `@context` as one entry of a list naming other contexts too.
///
/// # Errors
///
/// Returns [`CoreContextError`] when the configuration is missing or its URL is not an IRI.
pub fn core_entry() -> Result<ContextEntry, CoreContextError> {
    Ok(ContextEntry::Reference(IriRefBuf::from(core_iri()?)))
}

/// The core `@context` written as a one-element list: `"@context": ["<core>"]`.
///
/// # Errors
///
/// Returns [`CoreContextError`] when the configuration is missing or its URL is not an IRI.
pub fn core_context_list() -> Result<Context, CoreContextError> {
    Ok(Context::Multiple(vec![core_entry()?]))
}

/// The `Link` header naming the core `@context`, as an `application/json` answer carries it (clause 6.3.6):
/// `<core>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"`.
///
/// # Errors
///
/// Returns [`CoreContextError`] when the configuration is missing, its URL is not an IRI or it cannot be carried in a
/// header.
pub fn core_context_link() -> Result<Link, CoreContextError> {
    Ok(Link::try_json_ld_context(IriRefBuf::from(core_iri()?))?)
}

#[cfg(test)]
mod tests {
    use crate::core_context::{core_context_link, core_entry};
    use googletest::prelude::*;
    use ngsi_ld::model::ContextEntry;

    #[gtest]
    fn the_core_context_link_names_the_configured_core_context() -> Result<()> {
        match (core_context_link(), core_entry()) {
            (Ok(link), Ok(ContextEntry::Reference(core))) => verify_that!(link.context_urls().collect::<Vec<_>>(), elements_are![eq(&&core)]),
            (Err(_), Err(_)) => Ok(()),
            (link, entry) => fail!("the link {link:?} and the entry {entry:?} disagree on the configuration"),
        }
    }
}
