//! Context-source registration discovery policy (ETSI GS CIM 009 clause 5.10.2).

use crate::{
    RegistrationService,
    context_source_properties::compact_context_source_properties,
    error::RegistrationError,
    information_compaction::compact_information,
    registration_invariant::RegistrationInvariant,
    registration_validation_reason::RegistrationValidationReason,
};
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
};
use andromeda_ngsi_ld_model::{
    registration::ContextSourceRegistration,
    registration_matching::{EntitySpecification, RegistrationMatchSpecification},
};
use andromeda_ngsi_ld_query::{
    csr::compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter},
    filter::{expansion::expand_q_expr, json_key_attributes::JsonKeyAttributes, path::expanded_attribute_name, referenced_attributes::referenced_attributes},
    geo::compile as geo_compile,
    pagination::{CountPolicy, Pagination},
    projection::expansion::expand_attrs,
    temporal::window::TimeInterval,
    term_expander::TermExpander,
    type_selection::grammar::TypeExprRaw,
};
use andromeda_ngsi_ld_request::{
    pagination_cursor::decode_cursor,
    query::{SharedQueryParams, csr::CsrQueryParameters},
};
use iri_rs::IriBuf;
use serde_json::Value;

/// The registrations a discovery query answers with, and the pagination it
/// reports around them (clause 5.10.2).
pub struct DiscoveredRegistrations {
    /// The clause-5.10.2.5-trimmed, response-ready registration documents.
    pub registrations: Vec<Value>,
    /// Total matches before pagination (clause 6.3.13).
    pub total: u64,
    /// The first and last registration identifiers of the raw store page,
    /// ascending, from which a clause 5.5.9.1 cursor pointer is computed. Read
    /// from the untrimmed page so the walk continues past every registration the
    /// store scanned, not only the ones the clause-5.10.2.5 trim kept. Absent on
    /// an empty page.
    pub boundary: Option<(Identifier, Identifier)>,
    /// Whether the raw store page filled the limit -- the clause 6.3.10
    /// page-fullness signal a cursor walk uses in place of a total.
    pub page_full: bool,
}

impl RegistrationService {
    /// Discover the registrations matching `params` (clause 5.10.2), excluding
    /// the sources the request already passed through (clause 5.12).
    ///
    /// Each returned registration matches on at least one of its
    /// `RegistrationInfo` elements (clause 5.12) and is compacted under
    /// `context`.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::Validation`] when `params` is not a
    /// well-formed discovery request, names more than one entity type, or
    /// names an attribute that is not an IRI once expanded,
    /// [`RegistrationError::Query`] when the `q`, `csf`, geo or temporal
    /// filter cannot be compiled, [`RegistrationError::JsonLd`] when `context`
    /// cannot be loaded or a response cannot be compacted under it,
    /// [`RegistrationError::Internal`] when a matched registration cannot be
    /// serialised, and [`RegistrationError::Persistence`] when the store
    /// rejects the query.
    pub async fn query(
        &self,
        params: &CsrQueryParameters,
        encountered_sources: &[ContextSourcePseudonym],
        context: &ContextSpec,
    ) -> Result<DiscoveredRegistrations, RegistrationError> {
        params
            .validate()
            .map_err(|error| RegistrationError::Validation(RegistrationValidationReason::QueryParameters(error)))?;
        let compiled = self.compile_csr_query(params, encountered_sources, context).await?;
        let page = self.repo.query_compiled(&compiled).await?;
        // Clause 5.5.9.1: the cursor pointers are read from the raw store page,
        // before the clause-5.10.2.5 trim below drops the registrations whose
        // RegistrationInfo the query does not match. clone: the boundary
        // outlives the page the loop consumes.
        let boundary = page
            .registrations
            .first()
            .map(|registration| registration.id.clone())
            .zip(page.registrations.last().map(|registration| registration.id.clone()));
        let page_full = page.registrations.len() >= compiled.pagination.limit as usize;
        let total = page.total;
        let names = NameCompactor::new(&self.engine, context);
        let mut values = Vec::with_capacity(page.registrations.len());
        for registration in page.registrations {
            let Some(mut registration) = registration_matching_the_query(registration, &compiled)? else {
                continue;
            };
            let additional_properties = registration.additional_properties.take();
            let mut value = serde_json::to_value(registration).map_err(RegistrationError::Representation)?;
            if let Some(information) = value.get_mut("information") {
                compact_information(&names, information).await;
            }
            if let Some(properties) = additional_properties
                && let Some(compacted) = compact_context_source_properties(&names, properties).await?
                && let (Some(target), Some(source)) = (value.as_object_mut(), compacted.as_object())
            {
                target.extend(source.iter().map(|(key, member)| (key.clone(), member.clone()))); // clone: the response object owns compacted members after the temporary document is dropped.
            }
            values.push(value);
        }
        Ok(DiscoveredRegistrations {
            registrations: values,
            total,
            boundary,
            page_full,
        })
    }

    async fn compile_csr_query(
        &self,
        params: &CsrQueryParameters,
        encountered_sources: &[ContextSourcePseudonym],
        context: &ContextSpec,
    ) -> Result<CompiledCsrQuery, RegistrationError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);
        let expanded_type = match params.entity_type.as_ref() {
            Some(selection) => match selection.as_single_type() {
                Some(entity_type) => Some(expander.expand_type(entity_type.as_str()).await?),
                None => return Err(RegistrationError::Validation(RegistrationValidationReason::DiscoveryRequiresSingleType)),
            },
            None => None,
        };
        let mut attrs = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;
        let geo = geo_compile::compile_geo_query(params.geo.0.as_ref(), &expander).await?;
        // Clause 5.10.2.4: the attributes the `q` filter and the geoquery name
        // match registrations exactly as the `attrs` list does, so they join
        // it; the filter itself is not evaluated against a registration.
        if let Some(expression) = params.q.clone() {
            // clone: expansion owns and rewrites the request expression without mutating cached parameters.
            let expanded_filter = expand_q_expr(expression, &JsonKeyAttributes::default(), &expander).await?;
            for name in referenced_attributes(&expanded_filter) {
                let iri = IriBuf::new(name.as_str().to_owned())
                    .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::QNonIriAttribute { source }))?;
                if !attrs.contains(&iri) {
                    attrs.push(iri);
                }
            }
        }
        if let Some(geo) = &geo {
            let iri = IriBuf::new(geo.geoproperty.as_str().to_owned())
                .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::GeopropertyNotIri { source }))?;
            if !attrs.contains(&iri) {
                attrs.push(iri);
            }
        }
        let csf_expr = match params.csf.clone() {
            Some(expression) => Some(expand_q_expr(expression, &JsonKeyAttributes::default(), &expander).await?), // clone: same reason as `q`.
            None => None,
        };
        let temporal = if let (Some(timerel), Some(time_at)) = (&params.timerel, &params.time_at) {
            Some(
                CompiledCsrTemporalFilter::builder()
                    .interval(TimeInterval::from_parts(Some(*timerel), Some(*time_at), params.end_time_at)?)
                    .timeproperty(params.timeproperty)
                    .build(),
            )
        } else {
            None
        };

        Ok(CompiledCsrQuery::builder()
            .ids(params.id.clone().unwrap_or_default()) // clone: the compiled query owns request identifiers across the store await.
            .entity_type(expanded_type)
            .id_pattern(params.id_pattern.clone()) // clone: the compiled query owns the validated pattern.
            .attrs(attrs)
            .csf_expr(csf_expr)
            .geo(geo)
            .scope_q(params.scope_q.clone()) // clone: the compiled query owns the parsed scope query.
            .dataset_id(params.dataset_ids().map(<[_]>::to_vec).unwrap_or_default()) // clone: same reason.
            .encountered_sources(encountered_sources.to_vec()) // clone: the compiled query owns the Via listing across the store await.
            .temporal(temporal)
            .pagination(Pagination {
                limit: params.effective_limit(),
                offset: params.effective_offset(),
                count: CountPolicy::Omit,
            })
            // Clause 5.5.9.1: an opaque cursor, if the discovery carried one,
            // puts the store into keyset mode over the registration id.
            .keyset(
                decode_cursor(params.cursor.as_deref()).map_err(|error| RegistrationError::Validation(RegistrationValidationReason::QueryParameters(error)))?,
            )
            .build())
    }
}

/// The registration trimmed to the `RegistrationInfo` elements the compiled
/// query matches (clause 5.10.2.5), or nothing when none match.
///
/// The statement preselects on the clause 5.12 reading, evaluating the two
/// regular-expression cases on the portable restatements of the patterns, which
/// agree with the in-memory engine; the typed evaluation here is the authority,
/// and it is what settles the response.
///
/// Clause 5.10.2.5 states the trimming as a recommendation -- implementations
/// "should return filtered Context Source Registrations, which only contain
/// context source registration information relevant for the query, in
/// particular only matching `RegistrationInfo` elements". Andromeda adopts it as
/// behaviour: it answers what a discovery client actually asks (which parts of
/// a source are relevant to the query), and it keeps parity with the Context
/// Source Notification path, which trims on the identically worded clause
/// 5.11.7 recommendation. A registration none of whose elements match is
/// dropped entirely.
fn registration_matching_the_query(
    registration: ContextSourceRegistration,
    compiled: &CompiledCsrQuery,
) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
    let type_selector = compiled
        .entity_type
        .as_ref()
        .map(|iri| {
            iri.as_str()
                .parse::<EntityTypeName>()
                .map_err(|error| RegistrationError::from(RegistrationInvariant::ExpandedTypeName(error)))
        })
        .transpose()?
        .map(TypeExprRaw::Type);
    let entity = EntitySpecification {
        type_selector: type_selector.as_ref(),
        ids: &compiled.ids,
        id_pattern: compiled.id_pattern.as_ref(),
    };
    let entities = if compiled.entity_type.is_some() || !compiled.ids.is_empty() || compiled.id_pattern.is_some() {
        vec![entity]
    } else {
        Vec::new()
    };
    let attributes = compiled.attrs.iter().map(expanded_attribute_name).collect::<Result<Vec<AttributeName>, _>>()?;
    let specification = RegistrationMatchSpecification {
        entities: &entities,
        attributes: &attributes,
    };
    // Clause 5.10.2.5: keep only the RegistrationInfo elements the query
    // matches, and drop the whole registration when none remain.
    let mut registration = registration;
    registration.coverage.information.retain(|info| specification.matches_information(info));
    Ok((!registration.coverage.information.is_empty()).then_some(registration))
}
