//! The inputs a request referencing an `EntityMap` repeats beside the query it
//! compiles (clause 5.5.14).
//!
//! Clause 5.5.14 holds every request referencing an `EntityMap` to "the same
//! parameters as in the original request that created the `EntityMap`, except
//! for the specification of Entity identifiers or parameters related to
//! pagination, or, in the case of temporal requests, the temporal query". What
//! the compiled query selects is compared through its identity. This record
//! holds every other input clauses 5.7.1.3, 5.7.2.3, 5.7.3.3 and 5.7.4.3 give
//! the operation, and those a broker extension adds to it (Linked Entity
//! retrieval on a temporal request), so the two together are the request a map
//! was created for.
//!
//! The `@context` is one of those inputs, compared as the request references
//! it: clause 5.5.9.1 keeps "the same @context" throughout pagination and
//! answers "a different @context" with `BadRequestData`, and clause 5.5.9.3
//! makes the requests under a map pagination. The identifiers, `idPattern`,
//! `limit`, `offset`, `count`, the temporal query of clause 4.11 and the map's
//! own flag, lifetime and location are excepted, and so is every
//! representation choice that is no input of the operation (`format`,
//! `options`, the aggregation of a temporal representation). `lastN` is an
//! input of its own beside the temporal query, so it is not excepted with it.
//!
//! A list the operation reads as a set (`datasetId`, `expandValues`,
//! `jsonKeys`, `pick`, `omit`, `containedBy`) is recorded in one order
//! whatever order it was written in. `orderBy` and the language filter keep
//! theirs, since their order is their meaning.

use crate::{
    entity::GetEntityParams,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, ordering::parameters::OrderingParameters, projection::grammar::RawProjectionTerm};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use serde::Serialize;
use std::num::NonZeroU32;
use thiserror::Error;

/// The `@context` a request names, as the text it is processed under: the
/// context the request named in a `Link` header or in its body, with its core
/// context references removed and the core context last.
///
/// Two requests naming the same context share it whichever of the two
/// carriers they named it in, and a request naming another reference, a
/// mirror of the same context included, does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ContextReference<'a>(&'a str);

impl ContextReference<'_> {
    /// The reference whose processed text is `processed`.
    #[must_use]
    pub const fn new(processed: &str) -> ContextReference<'_> {
        ContextReference(processed)
    }
}

/// The inputs of a request that clause 5.5.14 holds a follow-up under an
/// `EntityMap` to, beside what its compiled query selects.
#[derive(Debug, Serialize)]
pub struct FollowUpInputs<'a> {
    /// The `@context` the request names.
    context: ContextReference<'a>,
    /// The clause 4.15 language filter, its ranges in their order of
    /// preference.
    lang: Option<&'a LanguageFilter>,
    /// The `datasetId`s, in one order.
    dataset_id: Vec<&'a DatasetSelector>,
    /// The Attributes whose values are expanded, in one order.
    expand_values: Vec<&'a AttributeName>,
    /// The Attributes whose values are raw JSON, in one order.
    json_keys: Vec<&'a AttributeName>,
    /// The clause 4.21 `pick` terms, in one order.
    pick: Vec<&'a RawProjectionTerm>,
    /// The clause 4.21 `omit` terms, in one order.
    omit: Vec<&'a RawProjectionTerm>,
    /// How Linked Entities are retrieved (clause 4.5.23).
    join: Option<JoinMode>,
    /// The depth Linked Entities are retrieved to.
    join_level: Option<NonZeroU32>,
    /// The Linked Entities already met, in one order.
    contained_by: Vec<&'a Identifier>,
    /// Whether split Entities are to be expected.
    split_entities: Option<bool>,
    /// The `GeoProperty` a `GeoJSON` representation takes its geometry from.
    geometry_property: Option<&'a AttributeName>,
    /// The ordering, its collation, location and geometry, for an operation
    /// that takes one.
    ordering: Option<&'a OrderingParameters>,
    /// The number of last instances of each Attribute a temporal request
    /// represents.
    last_n: Option<NonZeroU32>,
}

/// The inputs of a request could not be written as bytes.
#[derive(Debug, Error)]
#[error("the inputs of an entity map request cannot be written: {0}")]
pub struct FollowUpInputsError(#[from] serde_json::Error);

impl<'a> FollowUpInputs<'a> {
    /// The inputs of Query Entities through either binding, and of Create
    /// `EntityMap` for Query Entities, which takes the same (clauses 5.7.2.3
    /// and 5.14.4.3).
    #[must_use]
    pub fn of_entity_query(params: &'a EntityQueryParameters, context: ContextReference<'a>) -> FollowUpInputs<'a> {
        FollowUpInputs {
            context,
            lang: params.lang.as_ref(),
            dataset_id: datasets(params.dataset_id.as_deref()),
            expand_values: names(params.expand_values.as_deref()),
            json_keys: names(params.json_keys.as_deref()),
            pick: terms(params.pick.as_deref()),
            omit: terms(params.omit.as_deref()),
            join: params.join,
            join_level: params.join_level,
            contained_by: identifiers(params.contained_by.as_deref()),
            split_entities: params.split_entities,
            geometry_property: params.geometry_property.as_ref(),
            ordering: Some(&params.ordering),
            last_n: None,
        }
    }

    /// The inputs of Retrieve Entity (clause 5.7.1.3).
    #[must_use]
    pub fn of_entity_retrieval(params: &'a GetEntityParams, context: ContextReference<'a>) -> FollowUpInputs<'a> {
        FollowUpInputs {
            context,
            lang: params.lang.as_ref(),
            dataset_id: datasets(params.dataset_id.as_deref()),
            expand_values: Vec::new(),
            json_keys: Vec::new(),
            pick: terms(params.pick.as_deref()),
            omit: terms(params.omit.as_deref()),
            join: params.join,
            join_level: params.join_level,
            contained_by: identifiers(params.contained_by.as_deref()),
            split_entities: None,
            geometry_property: params.geometry_property.as_ref(),
            ordering: None,
            last_n: None,
        }
    }

    /// The inputs of Query Temporal Evolution of Entities through either
    /// binding, of its `EntityMap` creation and of Retrieve Temporal Evolution
    /// of an Entity (clauses 5.7.3.3, 5.7.4.3 and 5.14.5.3).
    #[must_use]
    pub fn of_temporal(params: &'a TemporalQueryParameters, context: ContextReference<'a>) -> FollowUpInputs<'a> {
        FollowUpInputs {
            context,
            lang: params.lang.as_ref(),
            dataset_id: datasets(params.dataset_id.as_deref()),
            expand_values: names(params.expand_values.as_deref()),
            json_keys: names(params.json_keys.as_deref()),
            pick: terms(params.pick.as_deref()),
            omit: terms(params.omit.as_deref()),
            join: params.join,
            join_level: params.join_level,
            contained_by: identifiers(params.contained_by.as_deref()),
            split_entities: params.split_entities,
            geometry_property: None,
            ordering: Some(&params.ordering),
            last_n: params.last_n,
        }
    }

    /// The canonical bytes of the record, which two requests share exactly
    /// when they carry the same inputs.
    ///
    /// # Errors
    ///
    /// [`FollowUpInputsError`] when a value cannot be written, which no value
    /// the request boundary admitted does.
    pub fn record(&self) -> Result<Vec<u8>, FollowUpInputsError> {
        Ok(serde_json::to_vec(self)?)
    }
}

/// `names` sorted and deduplicated.
fn names(names: Option<&[AttributeName]>) -> Vec<&AttributeName> {
    let mut names: Vec<&AttributeName> = names.unwrap_or_default().iter().collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// `terms` sorted and deduplicated.
fn terms(terms: Option<&[RawProjectionTerm]>) -> Vec<&RawProjectionTerm> {
    let mut terms: Vec<&RawProjectionTerm> = terms.unwrap_or_default().iter().collect();
    terms.sort_unstable();
    terms.dedup();
    terms
}

/// `identifiers` sorted by their text and deduplicated.
fn identifiers(identifiers: Option<&[Identifier]>) -> Vec<&Identifier> {
    let mut identifiers: Vec<&Identifier> = identifiers.unwrap_or_default().iter().collect();
    identifiers.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    identifiers.dedup();
    identifiers
}

/// `datasets` sorted by the text clause 4.5.5 writes them as and
/// deduplicated.
fn datasets(datasets: Option<&[DatasetSelector]>) -> Vec<&DatasetSelector> {
    let mut datasets: Vec<&DatasetSelector> = datasets.unwrap_or_default().iter().collect();
    datasets.sort_by_cached_key(ToString::to_string);
    datasets.dedup();
    datasets
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::GetEntityParams,
        follow_up_inputs::{ContextReference, FollowUpInputs, FollowUpInputsError},
        query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
    };

    /// The processed text of a request naming one context by reference.
    const CONTEXT: &str = r#"["https://example.org/context.jsonld","https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"]"#;

    fn entity_query(query: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(query).expect("a valid test fixture")
    }

    fn temporal_query(query: &str) -> TemporalQueryParameters {
        serde_urlencoded::from_str(query).expect("a valid test fixture")
    }

    fn retrieval(query: &str) -> GetEntityParams {
        serde_urlencoded::from_str(query).expect("a valid test fixture")
    }

    fn entity_record(query: &str) -> Result<Vec<u8>, FollowUpInputsError> {
        FollowUpInputs::of_entity_query(&entity_query(query), ContextReference::new(CONTEXT)).record()
    }

    fn temporal_record(query: &str) -> Result<Vec<u8>, FollowUpInputsError> {
        FollowUpInputs::of_temporal(&temporal_query(query), ContextReference::new(CONTEXT)).record()
    }

    fn retrieval_record(query: &str) -> Result<Vec<u8>, FollowUpInputsError> {
        FollowUpInputs::of_entity_retrieval(&retrieval(query), ContextReference::new(CONTEXT)).record()
    }

    /// Clause 5.5.14 excepts the specification of Entity identifiers, the
    /// pagination parameters and the map's own inputs, and the representation
    /// is no input of the operation.
    #[test]
    fn the_excepted_parameters_and_the_representation_are_no_inputs() -> Result<(), FollowUpInputsError> {
        let created = entity_record("type=Vehicle&entityMap=true&entityMapLifetime=PT1H")?;
        let follow_up = entity_record(
            "type=Building&id=urn:ngsi-ld:Vehicle:1&idPattern=urn:.*&limit=1&offset=3&count=true&options=keyValues&format=simplified&q=a%3D%3D1",
        )?;
        pretty_assertions::assert_eq!(created, follow_up);
        Ok(())
    }

    /// Clause 5.5.14 excepts the temporal query of clause 4.11 on a temporal
    /// request, and the aggregation of its representation is no input.
    #[test]
    fn the_temporal_query_and_the_aggregation_are_no_inputs() -> Result<(), FollowUpInputsError> {
        let created = temporal_record("type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z")?;
        let follow_up =
            temporal_record("type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z&timeproperty=modifiedAt&options=aggregatedValues&aggrMethods=sum")?;
        pretty_assertions::assert_eq!(created, follow_up);
        Ok(())
    }

    /// Every input clause 5.7.2.3 gives Query Entities outside its compiled
    /// query is part of the request a map was created for.
    #[test]
    fn each_input_of_a_query_is_part_of_the_record() -> Result<(), FollowUpInputsError> {
        let created = entity_record("type=Vehicle")?;
        for changed in [
            "type=Vehicle&lang=fr",
            "type=Vehicle&datasetId=urn:ngsi-ld:Dataset:a",
            "type=Vehicle&expandValues=brand",
            "type=Vehicle&jsonKeys=brand",
            "type=Vehicle&pick=id,brand",
            "type=Vehicle&omit=brand",
            "type=Vehicle&join=inline",
            "type=Vehicle&joinLevel=2",
            "type=Vehicle&containedBy=urn:ngsi-ld:Vehicle:1",
            "type=Vehicle&splitEntities=true",
            "type=Vehicle&geometryProperty=location",
            "type=Vehicle&orderBy=brand",
        ] {
            assert_ne!(created, entity_record(changed)?, "{changed}");
        }
        Ok(())
    }

    /// `lastN` is an input of its own beside the temporal query (clause
    /// 5.7.4.3), so clause 5.5.14 does not except it with the window.
    #[test]
    fn last_n_is_part_of_a_temporal_record() -> Result<(), FollowUpInputsError> {
        let created = temporal_record("type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z")?;
        assert_ne!(created, temporal_record("type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z&lastN=2")?);
        Ok(())
    }

    #[test]
    fn each_input_of_a_retrieval_is_part_of_the_record() -> Result<(), FollowUpInputsError> {
        let created = retrieval_record("")?;
        for changed in ["lang=fr", "datasetId=@none", "pick=id", "omit=brand", "join=flat", "geometryProperty=location"] {
            assert_ne!(created, retrieval_record(changed)?, "{changed}");
        }
        Ok(())
    }

    /// Clause 5.5.9.1 keeps one `@context` throughout pagination, which
    /// clause 5.5.9.3 makes the requests under a map.
    #[test]
    fn another_context_reference_is_another_record() -> Result<(), FollowUpInputsError> {
        let params = entity_query("type=Vehicle");
        let created = FollowUpInputs::of_entity_query(&params, ContextReference::new(CONTEXT)).record()?;
        let mirrored = FollowUpInputs::of_entity_query(
            &params,
            ContextReference::new(r#"["https://mirror.example.org/context.jsonld","https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"]"#),
        )
        .record()?;
        assert_ne!(created, mirrored);
        Ok(())
    }

    /// A list the operation reads as a set is the same input in any order,
    /// and a language filter's ranges are ranked by their order.
    #[test]
    fn a_set_is_one_input_in_any_order_and_a_language_filter_is_not() -> Result<(), FollowUpInputsError> {
        pretty_assertions::assert_eq!(
            entity_record("datasetId=urn:ngsi-ld:Dataset:a,urn:ngsi-ld:Dataset:b&pick=id,brand&expandValues=a,b&containedBy=urn:x:1,urn:x:2")?,
            entity_record("datasetId=urn:ngsi-ld:Dataset:b,urn:ngsi-ld:Dataset:a&pick=brand,id&expandValues=b,a&containedBy=urn:x:2,urn:x:1")?
        );
        assert_ne!(entity_record("lang=en,fr")?, entity_record("lang=fr,en")?);
        Ok(())
    }
}
