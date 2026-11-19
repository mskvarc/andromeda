//! Expansion of a written geo-query's `geoproperty` through a `@context`.

use crate::{
    geo::{geo_constraint::GeoConstraint, geo_query_error::GeoQueryError},
    name_stage::{Expanded, Written},
    query_error::QueryError,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, reference::NgsiLdReferenceError};

/// Expand the `geoproperty` of a written geo-query, leaving every other member
/// as written. An absent `geoproperty` stays absent: `location` is the default
/// of the reader, not a value to store.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when the term does not expand, and
/// [`GeoQueryError::InvalidGeoproperty`] when it expands past the identifier
/// bound.
pub async fn expand_geo_constraint(geo: &GeoConstraint<Written>, expander: &impl TermExpander) -> Result<GeoConstraint<Expanded>, QueryError> {
    let geoproperty = match &geo.geoproperty {
        Some(term) => {
            let iri = expander.expand_term(term.as_ref()).await?;
            Some(AttributeIri::try_from(iri).map_err(|too_long| GeoQueryError::InvalidGeoproperty(NgsiLdReferenceError::from(too_long)))?)
        }
        None => None,
    };
    Ok(GeoConstraint {
        // clone: the expanded constraint is owned apart from the written request
        georel: geo.georel.clone(),
        // clone: the expanded constraint is owned apart from the written request
        reference: geo.reference.clone(),
        geoproperty,
    })
}
