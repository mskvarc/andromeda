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

#[cfg(test)]
mod tests {
    use crate::{
        geo::{expansion::expand_geo_constraint, geo_constraint::GeoConstraint, geo_query_error::GeoQueryError},
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
        query_error::QueryError,
    };
    use andromeda_ngsi_ld_lexicon::{identifier_length::IDENTIFIER_BYTE_LIMIT, reference::NgsiLdReferenceError};
    use serde_json::json;

    /// The clause 5.2.13 geo-query of `geoproperty`, as a request writes it.
    fn constraint(geoproperty: Option<&str>) -> GeoConstraint<Written> {
        let mut members = json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [13.4, 52.5]});
        if let (Some(members), Some(geoproperty)) = (members.as_object_mut(), geoproperty) {
            members.insert("geoproperty".to_owned(), json!(geoproperty));
        }
        serde_json::from_value(members).expect("a readable geo-query")
    }

    #[tokio::test]
    async fn a_geoproperty_expands_to_the_iri_its_context_names_and_an_absent_one_stays_absent() {
        let expanded = expand_geo_constraint(&constraint(Some("area")), &EXAMPLE_NAMESPACE).await.expect("expands");
        pretty_assertions::assert_eq!(
            expanded.geoproperty.map(|name| name.as_str().to_owned()),
            Some("https://example.com/area".to_owned())
        );

        let default = expand_geo_constraint(&constraint(None), &EXAMPLE_NAMESPACE).await.expect("expands");
        assert!(default.geoproperty.is_none());
    }

    /// Clause 5.2.4 bounds an attribute name, and the expansion of the `geoproperty` may lengthen it.
    #[tokio::test]
    async fn a_geoproperty_expanded_past_the_identifier_bound_is_refused() {
        let term = "a".repeat(IDENTIFIER_BYTE_LIMIT - 4);

        let refused = expand_geo_constraint(&constraint(Some(&term)), &EXAMPLE_NAMESPACE).await;

        assert!(
            matches!(
                refused,
                Err(QueryError::GeoQuery(GeoQueryError::InvalidGeoproperty(NgsiLdReferenceError::TooLong(_))))
            ),
            "{refused:?}"
        );
    }
}
