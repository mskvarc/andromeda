//! The content a forwarded temporal write hands to the forwarding phase
//! (clauses 5.6.11.4, 5.6.12.4 and 5.6.14.4).

use andromeda_federation::forwarded_content::ForwardedContent;
use andromeda_temporal::forwarded_evolution::ForwardableEvolution;

/// `evolution` as forwarded content: a reified Scope (clause 4.5.6) rides with
/// the instances as the Scope paths of an entity do, whichever registration
/// receives them.
pub(crate) const fn forwarded_content(evolution: &ForwardableEvolution) -> ForwardedContent<'_> {
    match evolution.scope_history() {
        Some(scope_history) => ForwardedContent::Evolution {
            document: evolution.document(),
            scope_history,
        },
        None => ForwardedContent::Entity(evolution.document()),
    }
}
