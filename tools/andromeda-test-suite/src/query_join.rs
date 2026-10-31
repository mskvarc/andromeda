//! The `join` and `joinLevel` parameters (clause 4.5.23) added to a request whose operation writes none, such as Query
//! Temporal Evolution of Entities: the typed operation writes the rest of the query and the parameters follow it.

use http::{Uri, uri::InvalidUri};
use ngsi_ld::{http::UncheckedRequest, query::Join};

/// Why the parameters could not be added.
#[derive(Debug, thiserror::Error)]
#[error("the request target {target} with join={join}&joinLevel={level} is not a URI")]
pub struct JoinTargetError {
    /// The target the parameters were appended to.
    target: Uri,
    /// The `join` value appended.
    join: Join,
    /// The `joinLevel` value appended.
    level: u32,
    /// Why the joined text is not a URI.
    #[source]
    source: InvalidUri,
}

/// `request` with the parameters `join` and `joinLevel` appended to its query string.
///
/// # Errors
///
/// Returns [`JoinTargetError`] when the target with the parameters is not a URI.
pub fn with_join(request: UncheckedRequest, join: Join, level: u32) -> Result<UncheckedRequest, JoinTargetError> {
    let target = request.as_request().uri();
    let separator = if target.query().is_some() { '&' } else { '?' };
    let uri = format!("{target}{separator}join={join}&joinLevel={level}")
        .parse::<Uri>()
        .map_err(|source| JoinTargetError {
            // The error owns the target it names, borrowed from the request.
            target: target.clone(),
            join,
            level,
            source,
        })?;
    Ok(request.uri(uri))
}

#[cfg(test)]
mod tests {
    use crate::query_join::with_join;
    use googletest::prelude::*;
    use ngsi_ld::{
        http::{Endpoint, UncheckedRequest, operation::RetrieveEntityTypes},
        query::Join,
    };
    use url::Url;

    /// A request to the types listing of an unreachable broker.
    fn request() -> Result<UncheckedRequest> {
        let endpoint = Endpoint::builder().base_url(Url::parse("http://broker.test/ngsi-ld/v1/")?).build()?;
        Ok(UncheckedRequest::try_from_operation(&RetrieveEntityTypes::builder().build(), &endpoint)?)
    }

    #[gtest]
    fn the_parameters_start_the_query_of_a_target_without_one() -> Result<()> {
        let joined = with_join(request()?, Join::Inline, 1)?;
        verify_that!(
            joined.as_request().uri().to_string(),
            eq("http://broker.test/ngsi-ld/v1/types?join=inline&joinLevel=1")
        )
    }

    #[gtest]
    fn the_parameters_follow_the_query_of_a_target_that_has_one() -> Result<()> {
        let with_query = request()?.uri("http://broker.test/ngsi-ld/v1/types?details=true".parse()?);
        let joined = with_join(with_query, Join::Flat, 2)?;
        verify_that!(
            joined.as_request().uri().to_string(),
            eq("http://broker.test/ngsi-ld/v1/types?details=true&join=flat&joinLevel=2")
        )
    }
}
