//! A registration advertising `queryBatch` but not `queryEntity` (or a client
//! that queried via POST) is contacted through the batch query endpoint
//! (clause 6.23). Clause 6.23.1 makes it the same operation as the GET
//! binding, so it carries every parameter the narrowing wrote for the GET
//! binding, already spelled in the registration's forwarding context (clause
//! 4.3.6.6): the selection, the filters and the Linked Entity retrieval a `q`
//! is read under as members of the clause 5.2.23 [`Query`] body, and the page
//! (clause 6.3.10) and the `options` (clause 6.3.11), which the Query has no
//! member for, as URI parameters. Which filters travel is the narrowing's to
//! say: clause 5.7.2.4 forwards the client's filters unless split entities
//! apply.

use crate::{
    error::FederationError,
    federation_invariant::FederationInvariant,
    forwarded_names::forwarded_name,
    forwarded_parameter::ForwardedParameter,
    forwarded_query::ForwardedQuery,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity_selector::EntitySelector, query::body::Query};
use andromeda_ngsi_ld_query::{
    dataset_selector::DatasetSelector,
    filter::Filter,
    geo::geo_constraint::GeoConstraint,
    name_stage::Written,
    pattern::Pattern,
    query_error::QueryError,
    scope::scope_query::ScopeQuery,
    type_selection::TypeSelection,
};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use std::num::NonZeroU32;
use url::form_urlencoded;

/// A forwarded query as the batch binding sends it (clause 6.23.3.1).
#[derive(Debug)]
pub(crate) struct BatchQuery {
    /// The clause 5.2.23 Query the request body carries.
    pub(crate) body: Query,
    /// The URI parameters, `application/x-www-form-urlencoded`: the page and
    /// the `options`.
    pub(crate) parameters: String,
}

/// The batch binding's form of `query`, or `None` when its selection names
/// no entity, which happens when every identifier the client named fails the
/// client's own id pattern.
///
/// # Errors
///
/// Returns [`FederationError::Query`] when a narrowed type selection, id
/// pattern, `q`, `scopeQ`, `csf` or `datasetId` does not parse, and
/// [`FederationError::Internal`] when a narrowed name, identifier, geo-query,
/// language filter, `join` or `joinLevel` does not read back, or when the query
/// carries a parameter the batch binding has no place for, none of which the
/// narrowing produces.
pub(crate) fn batch_query(query: &ForwardedQuery) -> Result<Option<BatchQuery>, FederationError> {
    let mut gathered = Gathered::default();
    for (parameter, value) in query.parameters() {
        gathered.take(parameter, value)?;
    }
    gathered.into_batch_query()
}

/// The parameters of a forwarded query, gathered by the member they fill.
#[derive(Default)]
struct Gathered<'q> {
    /// The members that make the entity selectors.
    selection: Selection,
    /// The members that make the geo-query.
    geo: GeoMembers<'q>,
    /// The `attrs` member.
    attrs: Option<Vec<AttributeName>>,
    /// The `q` member.
    q: Option<Filter<Written>>,
    /// The `jsonKeys` member, naming what `q` reads as literal JSON keys.
    json_keys: Option<Vec<AttributeName>>,
    /// The `scopeQ` member.
    scope_q: Option<ScopeQuery>,
    /// The `csf` member.
    csf: Option<Filter<Written>>,
    /// The `datasetId` member.
    dataset_id: Option<Vec<DatasetSelector>>,
    /// The `lang` member.
    lang: Option<LanguageFilter>,
    /// The `join` member, written beside a `q` that crosses a Relationship.
    join: Option<JoinMode>,
    /// The `joinLevel` member.
    join_level: Option<NonZeroU32>,
    /// The parameters that travel in the URI, in the order they were written.
    uri: Vec<(ForwardedParameter, &'q str)>,
}

impl<'q> Gathered<'q> {
    /// Take one parameter into the member it fills.
    ///
    /// The match names every parameter, so a parameter a forwarded query may
    /// carry is never left out of the batch binding unnoticed. The temporal,
    /// representation and projection parameters and `containedBy` are not
    /// written by the narrowing of an entity query.
    ///
    /// # Errors
    ///
    /// As [`batch_query`] describes.
    fn take(&mut self, parameter: ForwardedParameter, value: &'q str) -> Result<(), FederationError> {
        match parameter {
            ForwardedParameter::Id => {
                self.selection.ids = value
                    .split(',')
                    .map(|id| {
                        id.parse::<Identifier>()
                            .map_err(|error| FederationError::from(FederationInvariant::ForwardedIdentifier(error)))
                    })
                    .collect::<Result<_, _>>()?;
            }
            ForwardedParameter::Type => self.selection.entity_type = Some(value.parse::<TypeSelection<Written>>().map_err(QueryError::from)?),
            ForwardedParameter::IdPattern => self.selection.id_pattern = Some(value.parse::<Pattern>().map_err(QueryError::from)?),
            ForwardedParameter::Limit | ForwardedParameter::Offset | ForwardedParameter::Options => self.uri.push((parameter, value)),
            ForwardedParameter::Attrs => self.attrs = Some(attribute_names(value)?),
            ForwardedParameter::Q => self.q = Some(value.parse::<Filter<Written>>().map_err(QueryError::from)?),
            ForwardedParameter::JsonKeys => self.json_keys = Some(attribute_names(value)?),
            ForwardedParameter::Georel => self.geo.georel = Some(value),
            ForwardedParameter::Geometry => self.geo.geometry = Some(value),
            ForwardedParameter::Coordinates => self.geo.coordinates = Some(value),
            ForwardedParameter::Geoproperty => self.geo.geoproperty = Some(forwarded_name::<AttributeName>(value)?),
            ForwardedParameter::ScopeQ => self.scope_q = Some(value.parse::<ScopeQuery>().map_err(QueryError::from)?),
            ForwardedParameter::Csf => self.csf = Some(value.parse::<Filter<Written>>().map_err(QueryError::from)?),
            ForwardedParameter::DatasetId => {
                self.dataset_id = Some(
                    value
                        .split(',')
                        .map(str::parse::<DatasetSelector>)
                        .collect::<Result<_, _>>()
                        .map_err(QueryError::from)?,
                );
            }
            ForwardedParameter::Lang => self.lang = Some(value.parse::<LanguageFilter>().map_err(FederationInvariant::ForwardedLanguageFilter)?),
            ForwardedParameter::Join => self.join = Some(value.parse::<JoinMode>().map_err(FederationInvariant::ForwardedJoin)?),
            ForwardedParameter::JoinLevel => self.join_level = Some(value.parse::<NonZeroU32>().map_err(FederationInvariant::ForwardedJoinLevel)?),
            ForwardedParameter::Format
            | ForwardedParameter::Timerel
            | ForwardedParameter::TimeAt
            | ForwardedParameter::EndTimeAt
            | ForwardedParameter::Timeproperty
            | ForwardedParameter::LastN
            | ForwardedParameter::AggrMethods
            | ForwardedParameter::AggrPeriodDuration
            | ForwardedParameter::ContainedBy
            | ForwardedParameter::ExpandValues
            | ForwardedParameter::Pick
            | ForwardedParameter::Omit
            | ForwardedParameter::EntityMapLifetime => return Err(FederationInvariant::BatchQueryParameter(parameter).into()),
        }
        Ok(())
    }

    /// The batch query the gathered members make, or `None` when the
    /// selection names no entity.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Internal`] when the geo-query members do
    /// not read back as a geo-query.
    fn into_batch_query(self) -> Result<Option<BatchQuery>, FederationError> {
        let Some(entities) = self.selection.into_selectors() else {
            return Ok(None);
        };
        let geo_q = self.geo.into_geo_query()?;
        let mut parameters = form_urlencoded::Serializer::new(String::new());
        for (parameter, value) in self.uri {
            parameters.append_pair(&parameter.to_string(), value);
        }
        let body = Query::builder()
            .entities(Some(entities))
            .attrs(self.attrs)
            .q(self.q)
            .json_keys(self.json_keys)
            .geo_q(geo_q)
            .scope_q(self.scope_q)
            .csf(self.csf)
            .dataset_id(self.dataset_id)
            .lang(self.lang)
            .join(self.join)
            .join_level(self.join_level)
            .build();
        Ok(Some(BatchQuery {
            body,
            parameters: parameters.finish(),
        }))
    }
}

/// The identifiers, type and id pattern a forwarded query selects by.
#[derive(Default)]
struct Selection {
    /// The identifiers, none when the query names none.
    ids: Vec<Identifier>,
    /// The type selection, `None` when the query names no type.
    entity_type: Option<TypeSelection<Written>>,
    /// The id pattern.
    id_pattern: Option<Pattern>,
}

impl Selection {
    /// The entity selectors of the Query: one per identifier the id pattern
    /// admits, each carrying the type, or a single selector with the type and
    /// the pattern when the query names no identifier; `None` when the pattern
    /// admits none of the identifiers.
    ///
    /// Clause 5.7.2.4 selects an entity whose id is one of the identifiers and
    /// matches the pattern, while within a selector `id` takes precedence over
    /// `idPattern` (clause 5.2.33), so the pattern selector's identifier axis
    /// decides which identifiers are posted rather than the pattern being sent
    /// beside them. A query naming no type selects every type (clause 4.17
    /// wildcard), since a selector's type is mandatory (Table 5.2.33-1).
    fn into_selectors(self) -> Option<Vec<EntitySelector<Written>>> {
        let pattern_selector = EntitySelector {
            id: None,
            id_pattern: self.id_pattern,
            type_: self.entity_type.unwrap_or(TypeSelection::Wildcard),
        };
        if self.ids.is_empty() {
            return Some(vec![pattern_selector]);
        }
        let admitted: Vec<EntitySelector<Written>> = self
            .ids
            .into_iter()
            .filter(|id| pattern_selector.identifier_axis().admits(id))
            .map(|id| EntitySelector {
                id: Some(vec![id]),
                id_pattern: None,
                type_: pattern_selector.type_.clone(), // clone: one selector per identifier names the same type
            })
            .collect();
        (!admitted.is_empty()).then_some(admitted)
    }
}

/// The clause 4.10 members a forwarded query writes for its geo-query.
#[derive(Default)]
struct GeoMembers<'q> {
    /// The `georel` text.
    georel: Option<&'q str>,
    /// The `geometry` text.
    geometry: Option<&'q str>,
    /// The `coordinates` JSON text.
    coordinates: Option<&'q str>,
    /// The `GeoProperty` the geo-query reads.
    geoproperty: Option<AttributeName>,
}

impl GeoMembers<'_> {
    /// The Table 5.2.13-1 `GeoQuery` the members make, its coordinates the
    /// JSON text they were written as, or `None` when the query carries no
    /// geo-query.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Internal`] when only some of `georel`,
    /// `geometry` and `coordinates` are written, or when they do not read
    /// back as a geo-query.
    fn into_geo_query(self) -> Result<Option<GeoConstraint<Written>>, FederationError> {
        match (self.georel, self.geometry, self.coordinates) {
            (Some(georel), Some(geometry), Some(coordinates)) => {
                let geo_query =
                    GeoConstraint::from_parameters(georel, geometry, coordinates, self.geoproperty).map_err(FederationInvariant::ForwardedGeoQuery)?;
                Ok(Some(geo_query))
            }
            (None, None, None) => Ok(None),
            (Some(_) | None, Some(_) | None, Some(_) | None) => Err(FederationInvariant::PartialForwardedGeoQuery.into()),
        }
    }
}

/// The attribute names of a comma-separated list the narrowing spelled.
///
/// # Errors
///
/// Returns [`FederationError::Internal`] when a name is no NGSI-LD name.
fn attribute_names(list: &str) -> Result<Vec<AttributeName>, FederationError> {
    list.split(',').map(forwarded_name::<AttributeName>).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        error::FederationError,
        federation_invariant::FederationInvariant,
        forwarded_batch_query::{BatchQuery, batch_query},
        forwarded_names::ForwardedNames,
        forwarded_parameter::ForwardedParameter,
        forwarded_query::{ForwardedQuery, client_query::ClientQuery},
        forwarding_context::Rewording,
        request::{SourcePage, TemporalAnswer},
        source_walk::SOURCE_PAGE_SIZE,
        split_entities::SplitEntities,
        term_resolver::TermResolver,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_query::entity_constraint::EntityConstraint;
    use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
    use serde_json::{Value, json};

    /// The information of a registration covering every Vehicle.
    fn vehicles() -> Value {
        json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}])
    }

    fn registration(information: &Value) -> ContextSourceRegistration {
        listing(information, None)
    }

    /// A registration of `information` whose `datasetId` member is `dataset_ids`.
    fn listing(information: &Value, dataset_ids: Option<&[&str]>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let Some(dataset_ids) = dataset_ids {
            document["datasetId"] = json!(dataset_ids);
        }
        serde_json::from_value(document).expect("valid registration")
    }

    /// The queries `client` is narrowed to for `by` under `split`, each
    /// source asked for `source_page`.
    async fn narrowed(client: ClientQuery<'_>, by: &ContextSourceRegistration, split: SplitEntities, source_page: Option<SourcePage>) -> Vec<ForwardedQuery> {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        ForwardedQuery::for_registration(client, by, split, source_page, TemporalAnswer::AsRequested, &names)
            .await
            .expect("narrowing succeeds")
    }

    /// The batch binding's form of the entity query `query_string` narrowed
    /// by `by` under `split`.
    async fn batched(by: &ContextSourceRegistration, query_string: &str, split: SplitEntities, source_page: Option<SourcePage>) -> Option<BatchQuery> {
        let params: EntityQueryParameters = serde_urlencoded::from_str(query_string).expect("valid query");
        let forwarded = narrowed(ClientQuery::EntityQuery(&params), by, split, source_page).await;
        batch_query(forwarded.first().expect("covered")).expect("the query builds")
    }

    /// The `Query` written for a forwarded query of `query_string` narrowed by
    /// a registration of `information`.
    async fn written(information: &Value, query_string: &str) -> Value {
        written_for(&registration(information), query_string).await
    }

    /// The `Query` written for a forwarded query of `query_string` narrowed by
    /// `by_type`.
    async fn written_for(by_type: &ContextSourceRegistration, query_string: &str) -> Value {
        let batch = batched(by_type, query_string, SplitEntities::Complete, None).await.expect("selects");
        serde_json::to_value(batch.body).expect("the query writes")
    }

    /// Clause 6.23 requires the first entity selector to carry both the type and the id, so a narrowed id+type query builds exactly that.
    #[tokio::test]
    async fn a_narrowed_id_and_type_query_builds_a_single_selector_with_both() {
        let value = written(&vehicles(), "type=Vehicle&id=urn:ngsi-ld:Vehicle:A1&attrs=speed").await;
        pretty_assertions::assert_eq!(value["type"], "Query");
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Vehicle");
        pretty_assertions::assert_eq!(value["entities"][0]["id"], "urn:ngsi-ld:Vehicle:A1");
        pretty_assertions::assert_eq!(value["attrs"], json!(["speed"]));
    }

    #[tokio::test]
    async fn a_type_only_query_builds_a_type_only_selector_and_no_attrs() {
        let value = written(&vehicles(), "type=Vehicle").await;
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Vehicle");
        assert!(value["entities"][0].get("id").is_none());
        assert!(value.get("attrs").is_none());
    }

    /// Several identifiers become one selector each, carrying the same type.
    #[tokio::test]
    async fn several_identifiers_build_one_selector_each() {
        let value = written(&vehicles(), "type=Vehicle&id=urn:ngsi-ld:Vehicle:1,urn:ngsi-ld:Vehicle:2").await;
        pretty_assertions::assert_eq!(value["entities"].as_array().map(Vec::len), Some(2));
        pretty_assertions::assert_eq!(value["entities"][1]["id"], "urn:ngsi-ld:Vehicle:2");
        pretty_assertions::assert_eq!(value["entities"][1]["type"], "Vehicle");
    }

    /// Clause 5.2.33: the body sent to a source holds only the selector of the
    /// client's union that the source's block covers.
    #[tokio::test]
    async fn a_union_query_writes_only_the_selector_the_block_covers() {
        let by_type = registration(&json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Beta"}]}]));
        let mut params: EntityQueryParameters = serde_urlencoded::from_str("").expect("valid query");
        params.entity_selectors = ["Alpha", "Beta"]
            .into_iter()
            .map(|raw| EntityConstraint {
                type_selection: Some(raw.parse().expect("valid type expression")),
                ..EntityConstraint::default()
            })
            .collect();
        let forwarded = narrowed(ClientQuery::EntityQuery(&params), &by_type, SplitEntities::Complete, None).await;
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        let batch = batch_query(&forwarded[0]).expect("the query builds").expect("selects");
        let value = serde_json::to_value(batch.body).expect("the query writes");
        pretty_assertions::assert_eq!(value["entities"].as_array().map(Vec::len), Some(1));
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Beta");
    }

    /// Clause 4.3.6.1 and Table 5.2.9-1: a batch query carries the `datasetId`
    /// a query string would, the client's ones the registration lists, every
    /// listed one when the client names none, and none when neither names any.
    #[tokio::test]
    async fn a_batch_query_names_only_the_dataset_ids_its_registration_lists() {
        let listing_default_and_a = listing(&vehicles(), Some(&["@none", "urn:ngsi-ld:Dataset:a"]));
        for (query, source, dataset_id) in [
            (
                "type=Vehicle&datasetId=urn:ngsi-ld:Dataset:b,urn:ngsi-ld:Dataset:a",
                &listing_default_and_a,
                Some(json!(["urn:ngsi-ld:Dataset:a"])),
            ),
            ("type=Vehicle", &listing_default_and_a, Some(json!(["@none", "urn:ngsi-ld:Dataset:a"]))),
            (
                "type=Vehicle&datasetId=urn:ngsi-ld:Dataset:b",
                &registration(&vehicles()),
                Some(json!(["urn:ngsi-ld:Dataset:b"])),
            ),
            ("type=Vehicle", &registration(&vehicles()), None),
        ] {
            let value = written_for(source, query).await;
            pretty_assertions::assert_eq!(value.get("datasetId").cloned(), dataset_id, "{query}");
        }
    }

    /// Clause 5.7.2.4 forwards the client's filters when split entities do not
    /// apply, and Table 5.2.23-1 has a member for each; clauses 6.3.10 and
    /// 6.3.11 carry the page and `sysAttrs` as URI parameters. The ranked
    /// language filter travels whole (clause 4.15).
    #[tokio::test]
    async fn a_complete_query_carries_every_filter_as_a_query_member() {
        let batch = batched(
            &registration(&vehicles()),
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&geoproperty=location&scopeQ=%2FMadrid&attrs=speed&csf=mode%3D%3D%22inclusive%22&lang=fr-CH%2Cfr%3Bq%3D0.9&options=sysAttrs",
            SplitEntities::Complete,
            None,
        )
        .await
        .expect("selects");
        let value = serde_json::to_value(&batch.body).expect("the query writes");
        pretty_assertions::assert_eq!(value["entities"], json!([{"type": "Vehicle"}]));
        pretty_assertions::assert_eq!(value["q"], "speed>50");
        pretty_assertions::assert_eq!(value["geoQ"]["georel"], "near;maxDistance==1000");
        pretty_assertions::assert_eq!(value["geoQ"]["geometry"], "Point");
        pretty_assertions::assert_eq!(value["geoQ"]["coordinates"], "[8.0,44.0]");
        pretty_assertions::assert_eq!(value["geoQ"]["geoproperty"], "location");
        pretty_assertions::assert_eq!(value["scopeQ"], "/Madrid");
        pretty_assertions::assert_eq!(value["attrs"], json!(["speed"]));
        pretty_assertions::assert_eq!(value["csf"], "mode==\"inclusive\"");
        pretty_assertions::assert_eq!(value["lang"], "fr-CH,fr;q=0.9");
        pretty_assertions::assert_eq!(batch.parameters, format!("limit={SOURCE_PAGE_SIZE}&options=sysAttrs"));
    }

    /// Clause 5.7.2.4: under split entities the filters wait for the
    /// aggregation and the batch binding leaves out what the GET binding
    /// leaves out, while the context source filter and the language filter
    /// still travel.
    #[tokio::test]
    async fn a_split_query_leaves_out_the_filters_and_keeps_csf_and_lang() {
        let batch = batched(
            &registration(&vehicles()),
            "type=Vehicle&q=speed%3E50&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B8.0%2C44.0%5D&scopeQ=%2FMadrid&attrs=speed&csf=mode%3D%3D%22inclusive%22&lang=en",
            SplitEntities::Split,
            None,
        )
        .await
        .expect("selects");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&batch.body).expect("the query writes"),
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "csf": "mode==\"inclusive\"", "lang": "en"})
        );
    }

    /// Clause 5.7.2.4 refuses a `q` over Linked Entity Attributes without a
    /// retrieval as deep as its hops, and Table 5.2.23-1 has a `join` and a
    /// `joinLevel` member, so the posted Query carries the retrieval the GET
    /// binding carries beside the `q`.
    #[tokio::test]
    async fn a_q_crossing_a_relationship_is_posted_with_its_join() {
        let batch = batched(
            &registration(&vehicles()),
            "type=Vehicle&q=isParkedIn%7Bcategory%7D%3D%3D%22commercial%22&join=flat&joinLevel=2&containedBy=urn:ngsi-ld:Building:1",
            SplitEntities::Complete,
            None,
        )
        .await
        .expect("selects");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&batch.body).expect("the query writes"),
            json!({
                "type": "Query",
                "entities": [{"type": "Vehicle"}],
                "q": "isParkedIn{category}==\"commercial\"",
                "join": "inline",
                "joinLevel": 2
            })
        );
        pretty_assertions::assert_eq!(batch.parameters, format!("limit={SOURCE_PAGE_SIZE}"));
    }

    /// Clause 4.9 EXAMPLE 11: `jsonKeys` names the members `q` reads as
    /// literal JSON keys, so it travels with the `q` it qualifies.
    #[tokio::test]
    async fn json_keys_travel_with_the_q_they_qualify() {
        let value = written(&vehicles(), "type=Vehicle&q=config%5Bmode%5D%3D%3D%22eco%22&jsonKeys=config").await;
        pretty_assertions::assert_eq!(value["q"], "config[mode]==\"eco\"");
        pretty_assertions::assert_eq!(value["jsonKeys"], "config");
    }

    /// A query naming no identifier selects by its id pattern.
    #[tokio::test]
    async fn an_id_pattern_travels_in_the_selector() {
        let value = written(&vehicles(), "type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:a.*").await;
        pretty_assertions::assert_eq!(value["entities"], json!([{"type": "Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:a.*"}]));
    }

    /// Clause 5.7.2.4 selects an entity whose id is listed and matches the
    /// pattern, and clause 5.2.33 lets a selector's `id` take precedence over
    /// its `idPattern`, so the identifiers the pattern refuses are left out,
    /// and a source asked for none of them is not contacted.
    #[tokio::test]
    async fn identifiers_the_client_pattern_refuses_are_left_out() {
        let by_type = registration(&vehicles());
        let batch = batched(
            &by_type,
            "type=Vehicle&id=urn:ngsi-ld:Vehicle:a1,urn:ngsi-ld:Vehicle:b1&idPattern=urn:ngsi-ld:Vehicle:a.*",
            SplitEntities::Complete,
            None,
        )
        .await
        .expect("selects a1");
        pretty_assertions::assert_eq!(
            serde_json::to_value(batch.body).expect("the query writes")["entities"],
            json!([{"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:a1"}])
        );
        assert!(
            batched(
                &by_type,
                "type=Vehicle&id=urn:ngsi-ld:Vehicle:b1&idPattern=urn:ngsi-ld:Vehicle:a.*",
                SplitEntities::Complete,
                None
            )
            .await
            .is_none()
        );
    }

    /// Clause 6.3.10: the page a range consumer asks each source for travels
    /// as URI parameters, its offset included.
    #[tokio::test]
    async fn the_page_a_range_consumer_asks_for_travels_in_the_uri() {
        let batch = batched(
            &registration(&vehicles()),
            "type=Vehicle",
            SplitEntities::Complete,
            Some(SourcePage { limit: 250, offset: 750 }),
        )
        .await
        .expect("selects");
        pretty_assertions::assert_eq!(batch.parameters, "limit=250&offset=750");
    }

    /// The narrowing of an entity query writes no temporal parameter, so one
    /// reaching the batch binding is a broken invariant rather than a
    /// parameter left out.
    #[tokio::test]
    async fn a_parameter_the_batch_binding_has_no_place_for_is_refused() {
        let params: TemporalQueryParameters = serde_urlencoded::from_str("type=Vehicle&timerel=before&timeAt=2024-01-01T00:00:00Z").expect("valid query");
        let forwarded = narrowed(ClientQuery::TemporalQuery(&params), &registration(&vehicles()), SplitEntities::Complete, None).await;
        let refused = batch_query(forwarded.first().expect("covered")).expect_err("a temporal parameter has no place");
        assert!(
            matches!(&refused, FederationError::Internal(invariant) if matches!(**invariant, FederationInvariant::BatchQueryParameter(ForwardedParameter::Timerel))),
            "{refused:?}"
        );
    }
}
