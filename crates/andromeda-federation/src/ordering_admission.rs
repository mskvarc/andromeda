//! Whether a read's answer may be ordered (clauses 4.23.1, 5.7.2.4, 5.7.4.4).
//!
//! Clause 5.7.2.4 and clause 5.7.4.4 both carry the same bullet: "If the
//! ordering parameter is present and the execution of the operation is not
//! limited to the local scope (see clause 5.5.13) then an error of type
//! BadRequestData shall be raised." Clause 4.23.1 states the reason from the
//! other side -- "Sort ordering is never applied to distributed operations" --
//! and opens by scoping the whole feature to "a Context Consumer [that] queries
//! for Entities retrieved from a single context broker".
//!
//! What makes an execution local is therefore read as clause 5.5.13 defines it:
//! "to only execute it based on information available in the Context Source or
//! Context Broker directly targeted by the request", of which the clause's
//! observable consequence is that "Context Source Registrations from the
//! Context Registry will not be used". A request may reach that state by asking
//! for it, and it may reach it because no registration matches the query and
//! the registry has nothing to contribute. Both leave the answer assembled from
//! one broker, which is the state clause 4.23.1 requires, so the ordering is
//! admissible in both. The refusal is kept for the case it was written for: a
//! registration takes part, and no broker can claim a total order over a result
//! set it only partly assembled.
//!
//! The narrowing bullet in the same two clauses -- a query naming no type,
//! attribute, filter or geoquery is refused "unless the execution of the
//! request is limited to local scope" -- is deliberately not read this way, and
//! `andromeda_ngsi_ld_request::query_narrowing` still decides it from the
//! request alone. That rule protects the broker from an unbounded query, and
//! clause 5.5.13 makes the local-scope mechanism the thing that "enables
//! broader local operations, e.g. pertaining to all entities, which would be
//! too expensive in the distributed case": the client asking for it is the
//! permission the broker needs, and permission cannot be inferred from an empty
//! registry. The ordering rule protects the answer's correctness instead, and
//! is satisfied by the fact of no distribution however that fact arose.

use crate::{FederationService, error::FederationError, participating_read::ParticipatingRead};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_query::ordering::parameters::OrderingParameters;
use andromeda_ngsi_ld_request::query_narrowing::ExecutionScope;
use derive_more::IsVariant;

/// Whether clause 4.23 ordering may be applied to a read's answer.
#[derive(Clone, Copy, Debug, Eq, IsVariant, PartialEq)]
pub enum OrderingAdmission {
    /// The answer is assembled from this broker alone, so it may be ordered.
    Admitted,
    /// A registered Context Source takes part in the read, and clause 4.23.1
    /// applies no sort ordering to a distributed operation.
    Refused,
}

/// The admission that needs no registry lookup.
///
/// A read that asked for no ordering has nothing to admit or refuse, and one
/// the request already limited to the local scope is the case clause 4.23.1
/// names outright. Everything else depends on what the registry holds.
fn admission_without_the_registry(ordering: &OrderingParameters, requested_scope: ExecutionScope) -> Option<OrderingAdmission> {
    if ordering.order_by.is_empty() || requested_scope == ExecutionScope::Local {
        return Some(OrderingAdmission::Admitted);
    }
    None
}

/// The admission a count of participating registrations settles.
const fn admission_of_participation(participating: usize) -> OrderingAdmission {
    if participating == 0 {
        OrderingAdmission::Admitted
    } else {
        OrderingAdmission::Refused
    }
}

impl FederationService {
    /// Whether `ordering`, the ordering `read` asked for, may be applied to
    /// its answer.
    ///
    /// The registry is consulted only for a read that asked for an ordering
    /// and did not already limit itself to the local scope, and it is asked
    /// which registrations take part in the read.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type cannot
    /// be expanded against the request's context, and
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read.
    pub async fn admit_ordering(
        &self,
        target: &ContextTarget,
        context: &ContextSpec,
        ordering: &OrderingParameters,
        read: &ParticipatingRead<'_>,
    ) -> Result<OrderingAdmission, FederationError> {
        if let Some(settled) = admission_without_the_registry(ordering, read.requested_scope) {
            return Ok(settled);
        }
        let participating = self.participating_registrations(target, context, read).await?.len();
        Ok(admission_of_participation(participating))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::ordering_admission::{OrderingAdmission, admission_of_participation, admission_without_the_registry};
    use andromeda_ngsi_ld_query::ordering::{parameters::OrderingParameters, term::OrderTerm};
    use andromeda_ngsi_ld_request::query_narrowing::ExecutionScope;
    use pretty_assertions::assert_eq;

    fn ordered_by(raw: &str) -> OrderingParameters {
        OrderingParameters {
            collation: None,
            geometry: None,
            order_by: vec![raw.parse::<OrderTerm>().expect("a valid ordering term")],
        }
    }

    fn unordered() -> OrderingParameters {
        OrderingParameters {
            collation: None,
            geometry: None,
            order_by: Vec::new(),
        }
    }

    #[test]
    fn a_read_that_asked_for_no_ordering_needs_no_registry_lookup() {
        assert_eq!(
            admission_without_the_registry(&unordered(), ExecutionScope::Distributed),
            Some(OrderingAdmission::Admitted)
        );
    }

    /// Clause 4.23.1: the case the rule was written for is settled by the
    /// request itself, without reading the registry.
    #[test]
    fn a_read_that_asked_for_the_local_scope_is_admitted_without_a_registry_lookup() {
        assert_eq!(
            admission_without_the_registry(&ordered_by("name"), ExecutionScope::Local),
            Some(OrderingAdmission::Admitted)
        );
    }

    #[test]
    fn an_ordered_distributed_read_is_left_to_the_registry() {
        assert_eq!(admission_without_the_registry(&ordered_by("name"), ExecutionScope::Distributed), None);
    }

    /// Clause 5.5.13: an execution no registration takes part in is executed
    /// "based on information available in the ... Context Broker directly
    /// targeted by the request", which is what clause 4.23.1 asks for.
    #[test]
    fn an_empty_registry_admits_the_ordering_and_one_participant_refuses_it() {
        assert_eq!(admission_of_participation(0), OrderingAdmission::Admitted);
        assert_eq!(admission_of_participation(1), OrderingAdmission::Refused);
        assert_eq!(admission_of_participation(7), OrderingAdmission::Refused);
    }
}
