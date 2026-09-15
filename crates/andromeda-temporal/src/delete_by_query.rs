//! Temporal history delete-by-query (an Andromeda extension; see
//! `docs/extensions/temporal-delete-by-query.md`).
//!
//! The operation resolves the entities its selection and window match with the
//! ordinary temporal query machinery, then removes the history instances the
//! attribute `drop`/`keep`, `datasetId`/`deleteAll` and time window scope. A
//! pre-count guards the [`TooManyResults`](TemporalError::TooManyResults) bound,
//! so an oversized delete is refused whole rather than half-executed, and a
//! `dryRun` returns that count without removing anything. The operation is
//! local-only: [`selection_params`](TemporalDeleteParameters::selection_params)
//! pins the local scope, so entity resolution never forwards.

use crate::{error::TemporalError, service::TemporalService};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    pagination::{CountPolicy, Pagination},
    temporal::{
        compiled::CompiledTemporalQuery,
        history_delete::{AttributeDeleteScope, TemporalHistoryDeleteScope},
    },
};
use andromeda_ngsi_ld_request::query::temporal::delete_parameters::TemporalDeleteParameters;
use iri_rs::IriBuf;

/// What one temporal history delete-by-query did.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TemporalDeleteOutcome {
    /// A `dryRun`: nothing was removed, and this is the count that would have
    /// been.
    DryRun {
        /// The history instances the delete would have removed.
        would_delete: u64,
    },
    /// The history instances that were removed.
    Deleted {
        /// The number of history instances removed.
        deleted: u64,
    },
}

impl TemporalService {
    /// Delete the temporal history instances a windowed delete-by-query selects.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Request`] when the parameters do not satisfy the
    /// delete's validation, [`TemporalError::JsonLd`] when a `drop`/`keep`
    /// attribute cannot be expanded against `context`,
    /// [`TemporalError::TooManyResults`] when the delete would remove more
    /// instances than the configured bound, and [`TemporalError::Persistence`]
    /// or [`TemporalError::Internal`] when the entity resolution or the store
    /// mutation fails.
    pub async fn delete_by_query(
        &self,
        target: &ContextTarget,
        params: &TemporalDeleteParameters,
        context: &ContextSpec,
    ) -> Result<TemporalDeleteOutcome, TemporalError> {
        params.validate()?;
        let selection = params.selection_params();
        let compiled = self.compile_temporal_query(&selection, context).await?;

        // Stage 1 entity resolution, unpaged: the delete addresses every
        // matching entity, not one page of them. A cheap guard first — no entity
        // matched means nothing to count or delete.
        let mut unpaged = compiled.clone();
        unpaged.pagination = Pagination {
            limit: u32::MAX,
            offset: 0,
            count: CountPolicy::Omit,
        };
        let outcome = self.reader.query_entity_ids(target, &unpaged).await?;
        let (entity_ids, _total) = outcome
            .complete(unpaged.pagination)
            .map_err(|error| TemporalError::internal(error.to_string()))?;
        if entity_ids.is_empty() {
            return Ok(empty_outcome(params));
        }

        let scope = self.compile_delete_scope(params, &compiled, context).await?;

        // The bound counts instances (what is removed), off the same predicate
        // the delete runs, so an oversized delete is refused before any row goes.
        let matched = self.mutator.count_history_instances(target, &entity_ids, &scope).await?;
        if matched > self.delete_by_query_max_matches {
            return Err(TemporalError::too_many_results(matched, self.delete_by_query_max_matches));
        }

        if params.is_dry_run() {
            return Ok(TemporalDeleteOutcome::DryRun { would_delete: matched });
        }

        let deleted = self.mutator.delete_history_instances(target, &entity_ids, &scope).await?;
        Ok(TemporalDeleteOutcome::Deleted { deleted })
    }

    /// Compile the neutral store scope from the delete's attribute, dataset and
    /// window parameters.
    async fn compile_delete_scope(
        &self,
        params: &TemporalDeleteParameters,
        compiled: &CompiledTemporalQuery,
        context: &ContextSpec,
    ) -> Result<TemporalHistoryDeleteScope, TemporalError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let attributes = match (params.drop.as_deref(), params.keep.as_deref()) {
            (Some(drop), _) => AttributeDeleteScope::Drop(self.expand_attributes(drop, &prepared_context).await?),
            (None, Some(keep)) => AttributeDeleteScope::Keep(self.expand_attributes(keep, &prepared_context).await?),
            (None, None) => AttributeDeleteScope::All,
        };
        // Validation has already refused `datasetId` beside `deleteAll`, so an
        // empty selector list here is exactly a bare delete or `deleteAll`, both
        // of which reduce to the unfiltered predicate.
        let dataset = DatasetFilter::from_selectors(params.dataset_id.as_deref().unwrap_or_default());
        Ok(TemporalHistoryDeleteScope {
            attributes,
            dataset,
            window: compiled.window,
            delete_all: params.deletes_all_datasets(),
        })
    }

    /// Expand a `drop`/`keep` attribute-name list to the IRIs the store matches.
    async fn expand_attributes(&self, names: &[AttributeName], prepared_context: &PreparedTermContext) -> Result<Vec<IriBuf>, TemporalError> {
        let mut expanded = Vec::with_capacity(names.len());
        for name in names {
            expanded.push(self.engine.expand_term(name.as_str(), prepared_context).await?);
        }
        Ok(expanded)
    }
}

/// The outcome when no entity matched: a dry run reports zero, a real delete
/// removed zero.
const fn empty_outcome(params: &TemporalDeleteParameters) -> TemporalDeleteOutcome {
    if params.is_dry_run() {
        TemporalDeleteOutcome::DryRun { would_delete: 0 }
    } else {
        TemporalDeleteOutcome::Deleted { deleted: 0 }
    }
}
