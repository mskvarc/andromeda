//! The parameters of Purge Entities (clause 5.6.21, bound to `DELETE /entities`
//! by clause 6.4.3.3).
//!
//! Table 6.4.3.3-1 lists what the operation reads: the entity selection (`id`,
//! `type`, `idPattern`, `q`, the geoquery, `scopeQ`, `csf`), `geometryProperty`,
//! and the `drop` and `keep` lists of Attribute names clause 5.6.21.3 gives it.
//! Its own cardinality rule names `attrs` as a narrowing input, clause 6.3.18
//! adds `local`, and the Andromeda `purge_dataset_scope` extension adds
//! `datasetId` and `deleteAll`, gated by the binding. A purge renders, pages
//! and counts nothing, so every other parameter of Query Entities is one the
//! operation does not define (clause 6.3.20).
//!
//! Clause 5.6.21.4 has the purge "run a Query Entities operation" to find its
//! entities, so the selection converts into the entity query parameters that
//! operation compiles.

use crate::{
    attribute_selection::{AttrsAdmission, validate_attrs},
    error::ParameterError,
    pagination::parameters::PageParameters,
    query::{
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_selectors,
            deserialize_optional_filter_expr,
            deserialize_optional_pattern,
            deserialize_optional_uri_csv,
        },
        entity::parameters::EntityQueryParameters,
    },
    query_narrowing::{AdmittedNarrowings, ExecutionScope, NarrowingInputs, require_narrowing, require_wildcard_locality},
};
use andromeda_linked_retrieval::admission::{LinkedOperation, LinkedRequest, admit_request};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    ordering::parameters::OrderingParameters,
    pattern::Pattern,
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExprRaw,
};
use serde::Deserialize;

/// Raw parameters of one Purge Entities request, in their compact form before
/// context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PurgeEntitiesParameters {
    /// Entity identifiers to select, comma-separated (`id`).
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Identifier>>,
    /// Entity types to select, as a type-selection expression (clause 4.17 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// A regular expression the selected identifiers must match (`idPattern`).
    #[serde(rename = "idPattern")]
    #[serde(default, deserialize_with = "deserialize_optional_pattern")]
    pub id_pattern: Option<Pattern>,
    /// Attributes a selected entity has, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// The `q` filter expression (clause 4.9), parsed at the request boundary.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// Complete clause-4.10 geo-query parsed at URL extraction.
    #[serde(flatten)]
    pub geo: OptionalGeoQuery,
    /// The scope query expression (clause 4.19 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<ScopeQuery>,
    /// The context source filter (clause 4.9 `csf`).
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub csf: Option<Expr>,
    /// The `GeoProperty` Table 6.4.3.3-1 names for a `GeoJSON` representation
    /// (`geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// Whether the operation is confined to this broker (clause 6.3.18 `local`).
    pub local: Option<bool>,
    /// Attributes removed from every selected entity (clause 5.6.21 `drop`).
    /// Mutually exclusive with `keep`.
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub drop: Option<Vec<AttributeName>>,
    /// Attributes every selected entity keeps, all others being removed
    /// (clause 5.6.21 `keep`). Mutually exclusive with `drop`.
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub keep: Option<Vec<AttributeName>>,
    /// The dataset instances a `drop`/`keep` purge removes (`datasetId`, the
    /// `purge_dataset_scope` extension).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Whether a `drop`/`keep` purge removes every `datasetId` instance of the
    /// attributes it targets (`deleteAll`, the `purge_dataset_scope`
    /// extension). Contradictory with `datasetId`.
    #[serde(rename = "deleteAll")]
    pub delete_all: Option<bool>,
}

/// A validated purge, split into the Query Entities selection clause 5.6.21.4
/// runs and the members that say what the purge removes.
#[derive(Debug)]
pub struct PurgeSelection {
    /// The selection, as the Query Entities operation compiles it.
    pub query: EntityQueryParameters,
    /// Attributes removed from every selected entity.
    pub drop: Option<Vec<AttributeName>>,
    /// Attributes every selected entity keeps.
    pub keep: Option<Vec<AttributeName>>,
    /// Whether every `datasetId` instance of the targeted attributes goes.
    pub delete_all: Option<bool>,
}

impl PurgeEntitiesParameters {
    /// The execution scope the request asked for (clause 5.5.13).
    #[must_use]
    pub const fn execution_scope(&self) -> ExecutionScope {
        ExecutionScope::of(self.local, self.entity_type.as_ref())
    }

    /// Validate the purge before its selection runs.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::SelectorRequired`] for a purge none of the
    /// clause 5.6.21.4 inputs narrows, [`ParameterError::AttrsReservedMember`]
    /// when `attrs` names an Entity member,
    /// [`ParameterError::WildcardTypeWithLocalFalse`] when `type=*` meets
    /// `local=false`, [`ParameterError::DropKeepExclusive`] when `drop` and
    /// `keep` are both present, [`ParameterError::DatasetIdWithDeleteAll`]
    /// when `datasetId` and `deleteAll` are, and
    /// [`ParameterError::LinkedRetrievalUnavailable`] when `q` traverses a
    /// Linked Entity, which clause 5.6.21.4 refuses.
    pub fn validate(&self) -> Result<(), ParameterError> {
        require_narrowing(
            &NarrowingInputs::builder()
                .admitted(AdmittedNarrowings::ENTITIES)
                .entity_type(self.entity_type.as_ref())
                .attrs(self.attrs.as_deref())
                .purged(self.drop.as_deref().or(self.keep.as_deref()))
                .q(self.q.as_ref())
                .geo(self.geo.0.as_ref())
                .local_scope(self.execution_scope())
                .build(),
        )?;
        validate_attrs(self.attrs.as_deref(), AttrsAdmission::AttributeNamesOnly)?;
        require_wildcard_locality(self.local, self.entity_type.as_ref())?;
        if self.drop.is_some() && self.keep.is_some() {
            return Err(ParameterError::DropKeepExclusive);
        }
        // Clause 5.6.13: `deleteAll` removes every instance while `datasetId`
        // names one, so the pair is contradictory. Whether either is admitted
        // at all is the extension gate the binding applies.
        if self.delete_all == Some(true) && self.dataset_id.is_some() {
            return Err(ParameterError::DatasetIdWithDeleteAll);
        }
        admit_request(LinkedOperation::PurgeEntities, &LinkedRequest::builder().q(self.q.as_ref()).build())?;
        Ok(())
    }

    /// Split the purge into the selection it runs and what it removes.
    #[must_use]
    pub fn into_selection(self) -> PurgeSelection {
        let query = EntityQueryParameters {
            id: self.id,
            entity_type: self.entity_type,
            id_pattern: self.id_pattern,
            entity_selectors: Vec::new(),
            attrs: self.attrs,
            q: self.q,
            geo: self.geo,
            page: PageParameters::default(),
            count: None,
            options: None,
            local: self.local,
            pick: None,
            omit: None,
            ordering: OrderingParameters::default(),
            scope_q: self.scope_q,
            lang: None,
            dataset_id: self.dataset_id,
            expand_values: None,
            json_keys: None,
            format: None,
            join: None,
            join_level: None,
            contained_by: None,
            entity_map: None,
            entity_map_lifetime: None,
            split_entities: None,
            geometry_property: self.geometry_property,
            csf: self.csf,
        };
        PurgeSelection {
            query,
            drop: self.drop,
            keep: self.keep,
            delete_all: self.delete_all,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::ParameterError, query::entity::purge::PurgeEntitiesParameters};
    use pretty_assertions::assert_eq;

    fn params(query: &str) -> PurgeEntitiesParameters {
        serde_urlencoded::from_str(query).expect("a decodable purge")
    }

    /// Table 6.4.3.3-1 with `attrs`, `local` and the dataset-scope extension.
    #[test]
    fn the_purge_parameters_are_read() {
        let p = params(
            "id=urn:ngsi-ld:Vehicle:1&type=Vehicle&idPattern=urn:.*&attrs=speed&q=speed%3E1&scopeQ=/Madrid&csf=tier==1&geometryProperty=location&local=true&drop=speed&datasetId=@none&georel=near%3BmaxDistance==1&geometry=Point&coordinates=[1,2]&geoproperty=location",
        );
        assert!(p.id.is_some() && p.entity_type.is_some() && p.id_pattern.is_some() && p.attrs.is_some());
        assert!(p.q.is_some() && p.scope_q.is_some() && p.csf.is_some() && p.geo.0.is_some());
        assert!(p.geometry_property.is_some() && p.drop.is_some() && p.dataset_id.is_some());
        assert_eq!(p.local, Some(true));
        assert!(params("type=Vehicle&keep=speed&deleteAll=true").keep.is_some());
    }

    /// A purge renders, pages and counts nothing, so each of these is a
    /// parameter the operation does not define (clause 6.3.20).
    #[test]
    fn every_query_only_parameter_is_unknown() {
        for parameter in [
            "pick=speed",
            "omit=speed",
            "lang=en",
            "format=keyValues",
            "options=sysAttrs",
            "count=true",
            "limit=3",
            "offset=1",
            "cursor=abc",
            "join=flat",
            "joinLevel=1",
            "containedBy=urn:ngsi-ld:Vehicle:1",
            "orderBy=id",
            "collation=en",
            "expandValues=speed",
            "jsonKeys=speed",
            "entityMap=true",
            "entityMapLifetime=PT1H",
            "splitEntities=true",
        ] {
            let message = serde_urlencoded::from_str::<PurgeEntitiesParameters>(&format!("type=Vehicle&{parameter}"))
                .expect_err(parameter)
                .to_string();
            assert!(message.contains("unknown field"), "{parameter}: {message}");
        }
    }

    /// Clause 5.6.21.4: each listed input narrows the purge on its own, and
    /// identifiers alone do not.
    #[test]
    fn a_purge_needs_a_narrowing_input() {
        for query in ["type=Vehicle", "attrs=speed", "drop=speed", "keep=speed", "q=speed%3E1", "local=true"] {
            assert!(params(query).validate().is_ok(), "{query}");
        }
        let err = params("id=urn:ngsi-ld:Vehicle:1").validate().expect_err("identifiers alone are too wide");
        assert!(matches!(err, ParameterError::SelectorRequired(_)), "{err:?}");
    }

    #[test]
    fn drop_and_keep_together_are_refused() {
        let err = params("type=Vehicle&drop=a&keep=b").validate().expect_err("drop and keep contradict");
        assert!(matches!(err, ParameterError::DropKeepExclusive), "{err:?}");
    }

    /// Clause 5.6.13: `deleteAll` and `datasetId` contradict each other, and
    /// each scopes a purge on its own.
    #[test]
    fn dataset_id_beside_delete_all_is_refused() {
        let err = params("type=Vehicle&drop=speed&datasetId=urn:ngsi-ld:Dataset:1&deleteAll=true")
            .validate()
            .expect_err("datasetId and deleteAll contradict");
        assert!(matches!(err, ParameterError::DatasetIdWithDeleteAll), "{err:?}");
        assert!(params("type=Vehicle&drop=speed&deleteAll=true").validate().is_ok());
        assert!(params("type=Vehicle&drop=speed&datasetId=urn:ngsi-ld:Dataset:1").validate().is_ok());
    }

    /// Table 5.2.33-1: `type=*` implies local scope.
    #[test]
    fn a_wildcard_type_is_local_and_refuses_local_false() {
        assert!(params("type=*").validate().is_ok());
        let err = params("type=*&local=false").validate().expect_err("type=* contradicts local=false");
        assert!(matches!(err, ParameterError::WildcardTypeWithLocalFalse), "{err:?}");
    }

    /// Clause 5.6.21.4: a filter on Linked Entity attributes is refused.
    #[test]
    fn a_q_hop_is_refused() {
        let err = params("type=Car&q=isParkedIn%7Bcategory%7D").validate().expect_err("purge retrieves nothing");
        assert!(matches!(err, ParameterError::LinkedRetrievalUnavailable { .. }), "{err:?}");
    }

    #[test]
    fn the_selection_keeps_what_selects_and_the_purge_keeps_what_it_removes() {
        let selection = params("type=Vehicle&q=speed%3E1&local=true&drop=speed&datasetId=@none").into_selection();
        assert!(selection.query.entity_type.is_some() && selection.query.q.is_some() && selection.query.dataset_id.is_some());
        assert_eq!(selection.query.local, Some(true));
        assert!(selection.drop.is_some() && selection.keep.is_none() && selection.delete_all.is_none());
    }
}
