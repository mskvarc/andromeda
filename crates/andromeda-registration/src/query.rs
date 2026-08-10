//! Context-source registration query policy.

use crate::{RegistrationService, context_source_properties::compact_context_source_properties, error::RegistrationError};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_query::{
    csr::compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter},
    filter::expansion::expand_q_expr,
    geo::compile as geo_compile,
    pagination::{CountPolicy, Pagination},
    projection::expansion::expand_attrs,
    temporal::window::TimeInterval,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, csr::CsrQueryParameters};
use serde_json::Value;

impl RegistrationService {
    /// Query registrations matching the supplied discovery parameters.
    pub async fn query(&self, params: &CsrQueryParameters, context: &ContextSpec) -> Result<(Vec<Value>, u64), RegistrationError> {
        params.validate().map_err(|error| RegistrationError::csr_validation(error.to_string()))?;
        let compiled = self.compile_csr_query(params, context).await?;
        let page = self.repo.query_compiled(&compiled).await?;
        let mut values = Vec::with_capacity(page.registrations.len());
        for mut registration in page.registrations {
            let additional_properties = registration.additional_properties.take();
            let mut value = serde_json::to_value(registration).map_err(|error| RegistrationError::internal(format!("Failed to serialize CSR: {error}")))?;
            if let Some(information) = value.get_mut("information") {
                self.compact_information(information, context).await?;
            }
            if let Some(properties) = additional_properties
                && let Some(compacted) = compact_context_source_properties(&self.engine, properties, context).await?
                && let (Some(target), Some(source)) = (value.as_object_mut(), compacted.as_object())
            {
                target.extend(source.iter().map(|(key, member)| (key.clone(), member.clone()))); // clone: the response object owns compacted members after the temporary document is dropped.
            }
            values.push(value);
        }
        Ok((values, page.total))
    }

    async fn compile_csr_query(&self, params: &CsrQueryParameters, context: &ContextSpec) -> Result<CompiledCsrQuery, RegistrationError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);
        let expanded_type = match params.entity_type.as_ref() {
            Some(selection) => match selection.as_single_type() {
                Some(entity_type) => Some(expander.expand_type(entity_type.as_str()).await?),
                None => return Err(RegistrationError::csr_validation("CSR discovery requires a single entity type")),
            },
            None => None,
        };
        let expanded_attrs = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;
        let geo = geo_compile::compile_geo_query(params.geo.0.as_ref(), &expander).await?;
        let q_expr = match params.q.clone() {
            Some(expression) => Some(expand_q_expr(expression, &expander).await?), // clone: compilation owns and rewrites the request expression without mutating cached parameters.
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
            .attrs(expanded_attrs)
            .q_expr(q_expr)
            .geo(geo)
            .temporal(temporal)
            .pagination(Pagination {
                limit: params.effective_limit(),
                offset: params.effective_offset(),
                count: CountPolicy::Omit,
            })
            .build())
    }
}
