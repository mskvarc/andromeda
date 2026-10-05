//! The `@context` a subscription is created or updated under.
//!
//! A client request names its own contexts (clause 6.3.5), and a
//! `jsonldContext` it leaves out is initialised from them, hosting a wrapper
//! when no single URL names them (clause 5.8.1.4, Annex C.8). A subscription
//! the broker places itself, such as the companion subscription a forwarded
//! subscription is joined through, is written under the core context alone and
//! has nothing to host.

use andromeda_jsonld::{compound_context_host::CompoundContextHost, context_spec::ContextSpec, user_context_admission::AdmittedContext};

/// The core context alone, which a subscription the broker places is written
/// under.
static CORE_ONLY: ContextSpec = ContextSpec::core_only();

/// The `@context` one subscription create or update is processed under.
#[derive(Clone, Copy)]
pub enum SubscriptionContext<'a> {
    /// A client request, and the tenant's host of a wrapper naming its
    /// contexts.
    Request {
        /// The contexts the request named, admitted (clause 5.5.7).
        context: &'a AdmittedContext,
        /// Where a wrapper of them is hosted when no single URL names them.
        host: &'a dyn CompoundContextHost,
    },
    /// A subscription the broker places itself, under the core context alone.
    CoreOnly,
}

impl SubscriptionContext<'_> {
    /// The contexts the subscription's terms are processed under.
    #[must_use]
    pub fn spec(&self) -> &ContextSpec {
        match self {
            SubscriptionContext::Request { context, .. } => context,
            SubscriptionContext::CoreOnly => &CORE_ONLY,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription_context::SubscriptionContext;
    use andromeda_jsonld::context_spec::{ContextSpec, CoreContextPolicy};

    #[test]
    fn a_broker_placed_subscription_is_processed_under_the_core_context_alone() {
        pretty_assertions::assert_eq!(SubscriptionContext::CoreOnly.spec(), &ContextSpec::core_only());
        pretty_assertions::assert_eq!(SubscriptionContext::CoreOnly.spec().core_policy(), CoreContextPolicy::Only);
    }
}
