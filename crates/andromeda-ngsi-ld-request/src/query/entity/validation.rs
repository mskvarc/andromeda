//! Entity-query semantic validation and option resolution.

use crate::{
    attribute_selection::{AttrsAdmission, validate_attrs},
    entity_map_lifetime::require_entity_map_for_lifetime,
    error::ParameterError,
    query::entity::{
        operation::EntityQueryOperation,
        parameters::{EntityQueryParameters, EntityQueryParametersOptions},
    },
    query_narrowing::{AdmittedNarrowings, ExecutionScope, NarrowingInputs, require_narrowing},
    representation_parameter::{resolve_representation, supported_option},
};
use andromeda_linked_retrieval::admission::{LinkedRequest, admit_request};
use andromeda_ngsi_ld_query::{
    projection::exclusivity::{Presence, ProjectionSelectors, check_projection_exclusivity},
    type_selection::grammar::TypeExprRaw,
};

impl EntityQueryParameters {
    /// The response shaping the `format` and `options` parameters selected.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::FormatInvalid`] or
    /// [`ParameterError::OptionsInvalid`] when either parameter carried a value
    /// Table 6.3.7-1 does not define (clause 6.3.20).
    pub fn parse_options(&self) -> Result<EntityQueryParametersOptions, ParameterError> {
        let options = supported_option(self.options.as_ref())?.unwrap_or_default();
        Ok(EntityQueryParametersOptions {
            render_mode: resolve_representation(self.format.as_ref(), self.options.as_ref())?,
            sys_attrs: options.sys_attrs,
            count: self.count.unwrap_or(false),
        })
    }

    /// Whether the query is confined to this broker: `local=true`, or a
    /// `type=*` selector, which Table 5.2.33-1 defines as implying local scope.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        self.local_flag_set() || self.selects_every_type()
    }

    /// The execution scope the request asked for (clause 5.5.13).
    #[must_use]
    pub const fn execution_scope(&self) -> ExecutionScope {
        if self.is_local() {
            ExecutionScope::Local
        } else {
            ExecutionScope::Distributed
        }
    }

    /// The clause 5.7.2.4 narrowing inputs this query carries.
    #[must_use]
    pub fn narrowing_inputs(&self) -> NarrowingInputs<'_> {
        NarrowingInputs::builder()
            .admitted(AdmittedNarrowings::ENTITIES)
            .entity_type(self.entity_type.as_ref())
            .entity_selectors(&self.entity_selectors)
            .attrs(self.attrs.as_deref())
            .purged(self.drop.as_deref().or(self.keep.as_deref()))
            .q(self.q.as_ref())
            .geo(self.geo.0.as_ref())
            .local_scope(self.execution_scope())
            .build()
    }

    /// Whether `local=true` was given.
    const fn local_flag_set(&self) -> bool {
        matches!(self.local, Some(true))
    }

    /// Whether the type selector is the clause 4.17 wildcard.
    const fn selects_every_type(&self) -> bool {
        matches!(self.entity_type, Some(TypeExprRaw::Wildcard))
    }

    /// Validate entity-query semantic constraints for `operation`.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::AttrsReservedMember`] when `attrs` names an
    /// Entity member, [`ParameterError::WildcardTypeWithLocalFalse`] when
    /// `type=*` is combined with `local=false`,
    /// [`ParameterError::DropKeepExclusive`] when `drop` and `keep` are both
    /// present, [`ParameterError::Projection`] for the clause 4.21 selector
    /// conflicts, [`ParameterError::LimitZeroWithoutCount`] for `limit=0`
    /// without the `count` option, the Linked Entity admission errors when a
    /// nested projection term or a `q` hop references a Linked Entity the
    /// operation does not reach (clauses 5.6.21.4, 5.7.2.4 and 5.14.4.4) or a
    /// join parameter meets none of the parameters it applies beside, and
    /// [`ParameterError::EntityMapLifetimeWithoutEntityMap`] when a query
    /// names a map lifetime without asking for a map.
    pub fn validate(&self, operation: EntityQueryOperation) -> Result<(), ParameterError> {
        // Clause 6.3.20: a `format` or `options` value Table 6.3.7-1 does not
        // define is refused here rather than during deserialisation, where the
        // error type would have been lost.
        self.parse_options()?;
        // Clause 5.7.2.4: a query naming no type, attribute, filter, geoquery
        // or local scope asks for everything the broker holds.
        require_narrowing(&self.narrowing_inputs())?;
        // Table 6.4.3.2-1 restricts `attrs` to "an Attribute (Property or
        // Relationship) name", which is the whole restriction whether or not
        // the row repeats `pick`'s exception. A query answers in an Entity
        // representation, where clause 4.18 leaves the Scope non-reified and so
        // not an Attribute at all, so `scope` names nothing the row admits.
        validate_attrs(self.attrs.as_deref(), AttrsAdmission::AttributeNamesOnly)?;
        // Table 5.2.33-1: `type=*` implies local scope, which the request may
        // not then contradict with `local=false`.
        if self.selects_every_type() && matches!(self.local, Some(false)) {
            return Err(ParameterError::WildcardTypeWithLocalFalse);
        }
        if self.drop.is_some() && self.keep.is_some() {
            return Err(ParameterError::DropKeepExclusive);
        }
        // Clause 5.6.13: `deleteAll` removes every instance while `datasetId`
        // names one, so the pair is contradictory. The check is unconditional;
        // whether `datasetId`/`deleteAll` are admitted at all is the
        // flag-dependent gate the handler applies.
        if self.delete_all == Some(true) && self.dataset_id.is_some() {
            return Err(ParameterError::DatasetIdWithDeleteAll);
        }
        check_projection_exclusivity(ProjectionSelectors {
            pick: Presence::of(self.pick.as_deref()),
            omit: Presence::of(self.omit.as_deref()),
            attrs: Presence::of(self.attrs.as_deref()),
        })?;
        let linked = LinkedRequest::builder()
            .pick(self.pick.as_deref())
            .omit(self.omit.as_deref())
            .q(self.q.as_ref())
            .join(self.join)
            .join_level(self.join_level)
            .contained_by(self.contained_by.as_deref())
            .build();
        admit_request(operation.linked(), &linked)?;
        match operation {
            EntityQueryOperation::Query | EntityQueryOperation::Purge => require_entity_map_for_lifetime(self.entity_map, self.entity_map_lifetime.as_ref()),
            EntityQueryOperation::EntityMap => Ok(()),
        }
    }
}
