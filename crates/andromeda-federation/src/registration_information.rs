//! Whether a registration's declared scope can answer a scoped read.

use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;

/// Whether a registration's declared scope can answer a scoped read.
pub(crate) fn registration_scope_admits(registration: &ContextSourceRegistration, scope_q: Option<&ScopeQuery>) -> bool {
    let Some(scope_q) = scope_q else {
        return true;
    };
    registration.coverage.scope.as_ref().is_none_or(|scopes| scope_q.matches(scopes))
}
