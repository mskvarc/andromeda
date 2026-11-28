//! Asking a source for the Scope event in force when a split temporal query's
//! window opened (clauses 4.18 and 5.7.4.4).
//!
//! Under `splitEntities` the scope filter is applied once the parts of an
//! entity are aggregated, and the Scope it reads includes the one in force when
//! the window opens (clause 4.18, Annex C clause C.5.16): the latest Scope
//! event before the start, across every part, unless that latest one is a
//! removal. The value and geo filters read the instances inside the window
//! alone (clause 5.7.4.4 steps S2 and S3), so only a scope query probes. The
//! forwarded window query returns the events inside the window only, so the
//! event a source held before the start is asked for with standard clause
//! 5.7.4 parameters alone. The latest removal is asked for on `deletedAt`,
//! where a removal carrying no instant on the window's axis is placed.

use crate::{
    FederationService,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    query_forwarding_error::QueryForwardingError,
    request::ForwardTemporalQueryRequest,
    source_reach::SourceReach,
    split_entities::SplitEntities,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;

/// Which Scope event before the window a probe asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InEffectReading {
    /// The latest event placed on the window's Temporal Property.
    Placed,
    /// The latest removal, placed on `deletedAt`.
    Deletion,
}

impl InEffectReading {
    /// The temporal query a source is sent for this probe of `client`, or
    /// `None` when the client's window opens where no Scope set before it can
    /// be in force ([`TemporalWindow::in_effect_start`]) or its filters read
    /// no Scope.
    ///
    /// The probe keeps the client's entity selection and context source
    /// filter. No filter travels, as under `splitEntities`. A scope query reads
    /// the Scope, which is no Attribute, and clause 5.7.4.4 selects an Entity by
    /// its Attribute instances alone, so `attrs` naming the Scope would select
    /// nothing: the probe names no `attrs`, and every attribute's latest
    /// instance and the Scope come back.
    ///
    /// [`TemporalWindow::in_effect_start`]: andromeda_ngsi_ld_query::temporal::window::TemporalWindow::in_effect_start
    #[must_use]
    pub fn parameters(self, client: &TemporalQueryParameters) -> Option<TemporalQueryParameters> {
        let start = client.window().ok().and_then(|window| window.in_effect_start())?;
        client.scope_q.as_ref()?;
        let timeproperty = match self {
            InEffectReading::Placed => client.timeproperty_or_default(),
            InEffectReading::Deletion => TimeProp::Deleted,
        };
        Some(TemporalQueryParameters {
            id: client.id.clone(),
            entity_type: client.entity_type.clone(),
            id_pattern: client.id_pattern.clone(),
            csf: client.csf.clone(),
            ..TemporalQueryParameters::latest_before(start, timeproperty)
        })
    }
}

impl FederationService {
    /// Forward the probe `reading` states of the temporal query `request`
    /// carries to the registrations that query is forwarded to; an empty
    /// outcome when the probe asks for nothing.
    ///
    /// None of the client's filters travels: the probe carries none.
    ///
    /// # Errors
    ///
    /// As [`FederationService::forward_temporal_query`].
    pub async fn forward_in_effect_probe(
        &self,
        target: &ContextTarget,
        request: ForwardTemporalQueryRequest<'_>,
        reading: InEffectReading,
    ) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError> {
        let Some(params) = reading.parameters(request.client_query) else {
            return Ok(ForwardReadOutcome::empty());
        };
        self.forward_temporal_query(
            target,
            ForwardTemporalQueryRequest {
                client_query: &params,
                split: SplitEntities::Complete,
                reach: SourceReach::Every,
                ..request
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        forwarded_names::ForwardedNames,
        forwarded_query::{ForwardedQuery, client_query::ClientQuery},
        forwarding_context::Rewording,
        in_effect_probe::InEffectReading,
        request::TemporalAnswer,
        split_entities::SplitEntities,
        term_resolver::TermResolver,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
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

    /// The query string the one registration is sent for `reading` of `query`,
    /// under the core context; `None` when the probe asks for nothing.
    async fn forwarded(query: &str, reading: InEffectReading) -> Option<String> {
        let params = reading.parameters(&client(query))?;
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

    /// Clause 4.18 and Table 6.18.3.2-1: a scope query probes for the last
    /// instance per attribute before the window opens, with no `attrs` because
    /// the Scope is no Attribute, and carries none of the filters.
    #[tokio::test]
    async fn a_scope_query_probes_the_latest_instance_before_the_window_start() {
        let query = forwarded(
            &format!("type=Vehicle&{WINDOW}&{FILTERS}&scopeQ=/Madrid&attrs=brand&datasetId=urn:d1"),
            InEffectReading::Placed,
        )
        .await
        .expect("a scope query over a window with a start is probed");
        for fragment in ["timerel=before", "timeAt=2020-08-01T12%3A00%3A00Z", "timeproperty=observedAt", "lastN=1"] {
            assert!(query.contains(fragment), "{fragment} missing: {query}");
        }
        for parameter in ["attrs=", "q=", "georel=", "geometry=", "scopeQ=", "endTimeAt=", "datasetId=", "brand"] {
            assert!(!query.contains(parameter), "{parameter} travels: {query}");
        }
    }

    /// Clause 4.5.7 places a removal without an instant on the window's axis at
    /// the instant it was deleted, so the deletion probe reads `deletedAt`.
    #[tokio::test]
    async fn a_deletion_probe_reads_on_deleted_at() {
        let query = forwarded(&format!("type=Vehicle&{WINDOW}&scopeQ=/Madrid"), InEffectReading::Deletion)
            .await
            .expect("probed");
        assert!(query.contains("timeproperty=deletedAt") && query.contains("lastN=1"), "{query}");
    }

    /// Clause 4.8: an instance carries its system times only when asked, and
    /// a probe on `modifiedAt` reads them, as clause 4.5.5.3 does wherever
    /// holders conflict, so every probe asks, the deletion probe on
    /// `deletedAt` included.
    #[tokio::test]
    async fn every_probe_asks_for_system_attributes() {
        let query = format!("type=Vehicle&{WINDOW}&timeproperty=modifiedAt&scopeQ=/Madrid");
        let placed = forwarded(&query, InEffectReading::Placed).await.expect("probed");
        assert!(placed.contains("timeproperty=modifiedAt") && placed.contains("options=sysAttrs"), "{placed}");
        let deleted = forwarded(&query, InEffectReading::Deletion).await.expect("probed");
        assert!(deleted.contains("timeproperty=deletedAt") && deleted.contains("options=sysAttrs"), "{deleted}");
    }

    /// The probe reaches the entities the client's query selects, so the
    /// identifiers, the identifier pattern and the context source filter
    /// travel as they do with the window query.
    #[tokio::test]
    async fn a_probe_keeps_the_selection_and_the_context_source_filter() {
        let query = forwarded(
            &format!("type=Vehicle&id=urn:ngsi-ld:Vehicle:1&idPattern=urn:ngsi-ld:Vehicle:.*&csf=region==%22north%22&{WINDOW}&scopeQ=/Madrid"),
            InEffectReading::Placed,
        )
        .await
        .expect("probed");
        for fragment in ["id=urn%3Angsi-ld%3AVehicle%3A1", "idPattern=urn%3Angsi-ld%3AVehicle%3A.*", "csf="] {
            assert!(query.contains(fragment), "{fragment} missing: {query}");
        }
    }

    /// Clause 5.7.4.4 steps S2 and S3: a query filtering by `q` or the
    /// geo-query reads the instances inside the window alone, so it probes for
    /// no instance before the window.
    #[tokio::test]
    async fn a_value_or_geo_filter_asks_for_no_instance_before_the_window() {
        for filters in [
            "q=speed>50",
            "georel=within&geometry=Polygon&coordinates=[[[13,52],[14,52],[14,53],[13,53],[13,52]]]",
            FILTERS,
        ] {
            for reading in [InEffectReading::Placed, InEffectReading::Deletion] {
                assert!(
                    forwarded(&format!("type=Vehicle&{WINDOW}&{filters}"), reading).await.is_none(),
                    "{filters} {reading:?}"
                );
            }
        }
    }

    /// A window with no start, or one on `deletedAt`, has no Scope in effect to
    /// ask for.
    #[tokio::test]
    async fn no_probe_for_before_or_unbounded_windows() {
        for query in [
            "type=Vehicle&timerel=before&timeAt=2020-08-01T12:00:00Z&scopeQ=/Madrid".to_owned(),
            "type=Vehicle&lastN=2&scopeQ=/Madrid".to_owned(),
            format!("type=Vehicle&{WINDOW}&timeproperty=deletedAt&scopeQ=/Madrid"),
            format!("type=Vehicle&{WINDOW}"),
        ] {
            assert!(forwarded(&query, InEffectReading::Placed).await.is_none(), "{query}");
        }
    }
}
