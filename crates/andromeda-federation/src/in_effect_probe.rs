//! Asking a source for the instances a split temporal query's filter
//! attributes held when its window opened (clauses 4.11 and 5.7.4.4).
//!
//! Under `splitEntities` the value, geo and scope filters are applied once the
//! parts of an entity are aggregated, and the instances they read include the
//! one in effect when the window opens (clause 4.11, Annex C clause C.5.5):
//! the latest instance of each stream before the start, across every part,
//! unless that latest one is a deletion. The forwarded window query returns
//! the instances inside the window only, so the instance a source held before
//! the start is asked for with standard clause 5.7.4 parameters alone, the
//! filter attributes as `attrs`. The latest deletion is asked for on
//! `deletedAt`, where a deletion carrying no instant on the window's axis is
//! placed. A source applying `lastN` per Attribute answers one dataset only,
//! so a probe may name one dataset. The latest deletion
//! is only the one with the latest `deletedAt`, so the probe is repeated to the
//! source that answered, with `timeAt` at the `deletedAt` it returned (clause
//! 4.11 makes `before` exclusive), for the deletions placed after the instance
//! in effect that it left out.

use crate::{
    FederationService,
    error::FederationError,
    federation_invariant::FederationInvariant,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    query_forwarding_error::QueryForwardingError,
    request::ForwardTemporalQueryRequest,
    source_reach::SourceReach,
    split_entities::SplitEntities,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReferenceError, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, filter::referenced_attributes::referenced_attributes, temporal::earlier_deletions::EarlierDeletions};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

/// Which instance before the window a probe asks for.
#[derive(Debug, Clone, Copy)]
pub enum InEffectReading<'a> {
    /// The latest instance placed on the window's Temporal Property.
    Placed,
    /// The latest deletion, placed on `deletedAt`.
    Deletion,
    /// The latest deletion before the `deletedAt` of the earliest one a source
    /// already answered, for the one stream the request names and to the one
    /// source it names.
    EarlierDeletion(&'a EarlierDeletions),
}

/// One probe for the instances in effect when a split query's window opens.
#[derive(Debug, Clone, Copy)]
pub struct InEffectProbe<'a> {
    /// Which instance the probe asks for.
    pub reading: InEffectReading<'a>,
    /// The one dataset the probe asks about; `None` asks across every dataset.
    /// An [`InEffectReading::EarlierDeletion`] names its own dataset.
    pub dataset: Option<&'a DatasetSelector>,
}

impl InEffectProbe<'_> {
    /// The temporal query a source is sent for this probe of `client`, or
    /// `None` when the client's window has no instance in effect before it
    /// ([`TemporalWindow::in_effect_start`]) or its filters read no attribute.
    ///
    /// The probe keeps the client's entity selection and context source
    /// filter, and reads the filter attributes alone: the first attribute
    /// every Query Term reads and the `GeoProperty` of the geo-query. No
    /// filter travels, as under `splitEntities`. A scope query reads the Scope,
    /// which is no Attribute, and clause 5.7.4.4 selects an Entity by its
    /// Attribute instances alone, so `attrs` naming the Scope would select
    /// nothing: a probe for a scope query names no `attrs`, and every
    /// attribute's latest instance and the Scope come back.
    ///
    /// # Errors
    ///
    /// Returns [`NgsiLdReferenceError`] when a core term the filters default
    /// to is not an attribute name.
    ///
    /// [`TemporalWindow::in_effect_start`]: andromeda_ngsi_ld_query::temporal::window::TemporalWindow::in_effect_start
    pub fn parameters(&self, client: &TemporalQueryParameters) -> Result<Option<TemporalQueryParameters>, NgsiLdReferenceError> {
        let Some(start) = client.window().ok().and_then(|window| window.in_effect_start()) else {
            return Ok(None);
        };
        let attributes = filter_attributes(client)?;
        if attributes.is_empty() && client.scope_q.is_none() {
            return Ok(None);
        }
        let attrs = client.scope_q.is_none().then_some(attributes);
        let timeproperty = match self.reading {
            InEffectReading::Placed => client.timeproperty_or_default(),
            InEffectReading::Deletion | InEffectReading::EarlierDeletion(_) => TimeProp::Deleted,
        };
        let params = TemporalQueryParameters {
            // clone: the probe owns the selection it forwards beside the client's query
            id: client.id.clone(),
            entity_type: client.entity_type.clone(),
            id_pattern: client.id_pattern.clone(),
            csf: client.csf.clone(),
            ..TemporalQueryParameters::latest_before(start, timeproperty, attrs, self.dataset.cloned()) // clone: the probe owns the dataset it names
        };
        Ok(Some(match self.reading {
            InEffectReading::Placed | InEffectReading::Deletion => params,
            InEffectReading::EarlierDeletion(earlier) => TemporalQueryParameters {
                // clone: the probe owns the stream it names
                id: Some(earlier.entities.clone()),
                id_pattern: None,
                attrs: Some(vec![earlier.attribute.clone()]),
                time_at: Some(earlier.before),
                dataset_id: Some(vec![earlier.dataset.clone()]),
                ..params
            },
        }))
    }

    /// The registrations the probe is sent to: the one source an earlier
    /// deletion follows up, every matching registration otherwise.
    const fn reach(&self) -> SourceReach<'_> {
        match self.reading {
            InEffectReading::Placed | InEffectReading::Deletion => SourceReach::Every,
            InEffectReading::EarlierDeletion(earlier) => SourceReach::Only(&earlier.source),
        }
    }
}

/// The attributes the value and geo filters of `client` read, in that order
/// and without repetition.
///
/// # Errors
///
/// Returns [`NgsiLdReferenceError`] when `location` is not an attribute name.
fn filter_attributes(client: &TemporalQueryParameters) -> Result<Vec<AttributeName>, NgsiLdReferenceError> {
    let mut attributes: Vec<AttributeName> = client.q.as_ref().map(referenced_attributes).unwrap_or_default().into_iter().cloned().collect(); // clone: the probe owns the names it forwards
    if let Some(geo) = client.geo.0.as_ref() {
        let geoproperty = match &geo.geoproperty {
            Some(reference) => AttributeName::from(reference),
            // Clause 4.10: a geo-query naming no `geoproperty` reads `location`.
            None => core_attribute(compact_properties::LOCATION)?,
        };
        attributes.push(geoproperty);
    }
    let mut distinct: Vec<AttributeName> = Vec::with_capacity(attributes.len());
    for attribute in attributes {
        if !distinct.contains(&attribute) {
            distinct.push(attribute);
        }
    }
    Ok(distinct)
}

/// The attribute a core term names, spelled as the term: the core context
/// applies under every context a probe is forwarded in, so the term expands to
/// the core IRI wherever it travels (clause 4.4).
///
/// # Errors
///
/// Returns [`NgsiLdReferenceError`] when it is not one.
fn core_attribute(term: &str) -> Result<AttributeName, NgsiLdReferenceError> {
    term.parse()
}

impl FederationService {
    /// Forward `probe` of the temporal query `request` carries to the
    /// registrations that query is forwarded to, each sent the probe narrowed
    /// to what it covers; an empty outcome when the probe asks for nothing.
    ///
    /// The probe's `attrs` are its own rather than the client's filter, so a
    /// block is sent the filter attributes it covers, and none of the client's
    /// filters travels: the probe carries none.
    ///
    /// # Errors
    ///
    /// As [`FederationService::forward_temporal_query`], and
    /// [`QueryForwardingError::Federation`] when a core term the filters
    /// default to is not an attribute name.
    pub async fn forward_in_effect_probe(
        &self,
        target: &ContextTarget,
        request: ForwardTemporalQueryRequest<'_>,
        probe: InEffectProbe<'_>,
    ) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError> {
        let Some(params) = probe
            .parameters(request.client_query)
            .map_err(|error| QueryForwardingError::from(FederationError::from(FederationInvariant::CoreAttributeName(error))))?
        else {
            return Ok(ForwardReadOutcome::empty());
        };
        self.forward_temporal_query(
            target,
            ForwardTemporalQueryRequest {
                client_query: &params,
                split: SplitEntities::Complete,
                reach: probe.reach(),
                ..request
            },
        )
        .await
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        forwarded_names::ForwardedNames,
        forwarded_query::{ForwardedQuery, client_query::ClientQuery},
        forwarding_context::Rewording,
        in_effect_probe::{InEffectProbe, InEffectReading},
        request::TemporalAnswer,
        source_reach::SourceReach,
        split_entities::SplitEntities,
        term_resolver::TermResolver,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_query::{dataset::DatasetSelector, temporal::earlier_deletions::EarlierDeletions};
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
    use serde_json::json;

    const WINDOW: &str = "timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T13:00:00Z";
    const FILTERS: &str = "q=speed>50&georel=within&geometry=Polygon&coordinates=[[[13,52],[14,52],[14,53],[13,53],[13,52]]]";

    fn registration() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn client(query: &str) -> TemporalQueryParameters {
        serde_urlencoded::from_str(query).expect("a valid temporal query")
    }

    /// The query string the one registration is sent for `probe` of `query`,
    /// under the core context; `None` when the probe asks for nothing.
    async fn forwarded(query: &str, probe: InEffectProbe<'_>) -> Option<String> {
        let params = probe.parameters(&client(query)).expect("the core terms are attribute names")?;
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        let queries = ForwardedQuery::for_registration(
            ClientQuery::TemporalQuery(&params),
            &registration(),
            SplitEntities::Complete,
            None,
            TemporalAnswer::AsRequested,
            &names,
        )
        .await
        .expect("the probe narrows");
        let [query] = queries.as_slice() else {
            panic!("one block, one query: {queries:?}");
        };
        Some(query.to_query_string())
    }

    const fn placed() -> InEffectProbe<'static> {
        InEffectProbe {
            reading: InEffectReading::Placed,
            dataset: None,
        }
    }

    /// Clause 4.11 and Table 6.18.3.2-1: the probe asks for the last instance
    /// per attribute before the window opens, of the filter attributes alone,
    /// and carries none of the filters.
    #[tokio::test]
    async fn a_split_probe_asks_for_the_latest_instance_before_the_window_start() {
        let query = forwarded(&format!("type=Vehicle&{WINDOW}&{FILTERS}&attrs=brand&datasetId=urn:d1"), placed())
            .await
            .expect("a window with a start is probed");
        for fragment in [
            "attrs=speed%2Clocation",
            "timerel=before",
            "timeAt=2020-08-01T12%3A00%3A00Z",
            "timeproperty=observedAt",
            "lastN=1",
        ] {
            assert!(query.contains(fragment), "{fragment} missing: {query}");
        }
        for parameter in ["q=", "georel=", "geometry=", "scopeQ=", "endTimeAt=", "datasetId=", "brand"] {
            assert!(!query.contains(parameter), "{parameter} travels: {query}");
        }
    }

    /// Clause 5.7.4.4 selects an Entity by its Attribute instances, and the
    /// Scope is none, so a probe for a scope query names no `attrs` and the
    /// Scope comes back beside every attribute's latest instance.
    #[tokio::test]
    async fn a_scope_query_probes_every_attribute() {
        let query = forwarded(&format!("type=Vehicle&{WINDOW}&q=speed>50&scopeQ=/Madrid"), placed())
            .await
            .expect("a scope query is probed");
        assert!(!query.contains("attrs=") && !query.contains("scopeQ") && query.contains("lastN=1"), "{query}");
        let scope_only = forwarded(&format!("type=Vehicle&{WINDOW}&scopeQ=/Madrid"), placed())
            .await
            .expect("a scope query is probed");
        assert!(!scope_only.contains("attrs="), "{scope_only}");
    }

    /// Clause 4.5.7 places a deletion without an instant on the window's
    /// axis at the instant it was deleted, so the deletion probe reads
    /// `deletedAt`.
    #[tokio::test]
    async fn a_deletion_probe_reads_on_deleted_at() {
        let deletion = InEffectProbe {
            reading: InEffectReading::Deletion,
            dataset: None,
        };
        let query = forwarded(&format!("type=Vehicle&{WINDOW}&q=speed>50"), deletion).await.expect("probed");
        assert!(query.contains("timeproperty=deletedAt") && query.contains("lastN=1"), "{query}");
    }

    /// Clause 4.11 makes `before` exclusive, so the request repeated for a
    /// stream starts at the `deletedAt` of the earliest deletion answered, names
    /// the Entities, the Attribute and the dataset of that stream alone, and
    /// is sent to its source only.
    #[tokio::test]
    async fn an_earlier_deletion_probe_names_its_stream_and_its_source() {
        let earlier = EarlierDeletions {
            source: "urn:ngsi-ld:csr:1".parse().expect("a valid identifier"),
            entities: vec!["urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")],
            attribute: "speed".parse().expect("a valid attribute name"),
            dataset: DatasetSelector::Default,
            before: "2020-08-01T11:50:00Z".parse().expect("a valid instant"),
        };
        let probe = InEffectProbe {
            reading: InEffectReading::EarlierDeletion(&earlier),
            dataset: None,
        };
        pretty_assertions::assert_eq!(probe.reach(), SourceReach::Only(&earlier.source));
        let query = forwarded(&format!("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:.*&{WINDOW}&q=speed>50&attrs=brand"), probe)
            .await
            .expect("probed");
        for fragment in [
            "id=urn%3Angsi-ld%3AVehicle%3A1",
            "attrs=speed",
            "timerel=before",
            "timeAt=2020-08-01T11%3A50%3A00Z",
            "timeproperty=deletedAt",
            "lastN=1",
            "datasetId=%40none",
        ] {
            assert!(query.contains(fragment), "{fragment} missing: {query}");
        }
        for parameter in ["idPattern=", "brand", "endTimeAt="] {
            assert!(!query.contains(parameter), "{parameter} travels: {query}");
        }
    }

    /// Clause 5.7.4.4 filters by `datasetId` before the `lastN` cut, so a
    /// follow-up naming one dataset reads that dataset's latest instance.
    #[tokio::test]
    async fn a_dataset_follow_up_names_the_dataset() {
        let dataset = DatasetSelector::Id("urn:ngsi-ld:dataset:d2".parse().expect("a valid identifier"));
        let probe = InEffectProbe {
            reading: InEffectReading::Placed,
            dataset: Some(&dataset),
        };
        let query = forwarded(&format!("type=Vehicle&{WINDOW}&q=speed>50"), probe).await.expect("probed");
        assert!(query.contains("datasetId=urn%3Angsi-ld%3Adataset%3Ad2"), "{query}");
    }

    /// A window with no start, one on `deletedAt`, or a query reading no
    /// attribute through its filters has no instance in effect to ask for.
    #[tokio::test]
    async fn no_probe_for_before_or_unbounded_windows() {
        for query in [
            "type=Vehicle&timerel=before&timeAt=2020-08-01T12:00:00Z&q=speed>50".to_owned(),
            "type=Vehicle&lastN=2&q=speed>50".to_owned(),
            format!("type=Vehicle&{WINDOW}&timeproperty=deletedAt&q=speed>50"),
            format!("type=Vehicle&{WINDOW}"),
        ] {
            assert!(forwarded(&query, placed()).await.is_none(), "{query}");
        }
    }

    /// Clause 4.8: an instance carries its system times only when asked, so a
    /// probe on `modifiedAt` asks for them, and the deletion probe, on
    /// `deletedAt`, does not.
    #[tokio::test]
    async fn a_probe_on_a_system_time_asks_for_system_attributes() {
        let query = format!("type=Vehicle&{WINDOW}&timeproperty=modifiedAt&q=speed>50");
        let placed = forwarded(&query, placed()).await.expect("probed");
        assert!(placed.contains("timeproperty=modifiedAt") && placed.contains("options=sysAttrs"), "{placed}");
        let deletion = InEffectProbe {
            reading: InEffectReading::Deletion,
            dataset: None,
        };
        let deleted = forwarded(&query, deletion).await.expect("probed");
        assert!(deleted.contains("timeproperty=deletedAt") && !deleted.contains("sysAttrs"), "{deleted}");
    }

    /// The probe reaches the entities the client's query selects, so the
    /// identifiers, the identifier pattern and the context source filter
    /// travel as they do with the window query.
    #[tokio::test]
    async fn a_probe_keeps_the_selection_and_the_context_source_filter() {
        let query = forwarded(
            &format!("type=Vehicle&id=urn:ngsi-ld:Vehicle:1&idPattern=urn:ngsi-ld:Vehicle:.*&csf=region==%22north%22&{WINDOW}&q=speed>50"),
            placed(),
        )
        .await
        .expect("probed");
        for fragment in ["id=urn%3Angsi-ld%3AVehicle%3A1", "idPattern=urn%3Angsi-ld%3AVehicle%3A.*", "csf="] {
            assert!(query.contains(fragment), "{fragment} missing: {query}");
        }
    }

    /// Clause 4.10 lets the geo-query name any `GeoProperty`, and a Query Term
    /// crossing a hop reads the Relationship it starts from (clause 4.9), so
    /// the probe asks for those.
    #[tokio::test]
    async fn a_named_geoproperty_and_a_hop_relationship_are_probed() {
        let query = forwarded(
            &format!(
                "type=Vehicle&{WINDOW}&q=parkedIn{{free}}>0&georel=within&geometry=Polygon&coordinates=[[[13,52],[14,52],[14,53],[13,53],[13,52]]]&geoproperty=position"
            ),
            placed(),
        )
        .await
        .expect("probed");
        assert!(query.contains("attrs=parkedIn%2Cposition"), "{query}");
    }

    /// Clause 4.5.5: the default instance is addressed as `@none`, so a
    /// follow-up for it names that.
    #[tokio::test]
    async fn a_default_dataset_follow_up_names_none() {
        let probe = InEffectProbe {
            reading: InEffectReading::Placed,
            dataset: Some(&DatasetSelector::Default),
        };
        let query = forwarded(&format!("type=Vehicle&{WINDOW}&q=speed>50"), probe).await.expect("probed");
        assert!(query.contains("datasetId=%40none"), "{query}");
    }
}
