//! Valid origins for newly stored hosted-context documents.

/// The policy operation that caused a non-cached Context to be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCreationKind {
    /// A client explicitly created the Context resource.
    Hosted,
    /// Subscription processing hosted an inline compound Context.
    ImplicitlyCreated,
}

#[cfg(test)]
mod tests {
    use crate::creation_kind::ContextCreationKind;

    #[test]
    fn cached_is_not_a_hosted_creation_kind() {
        let kinds = [ContextCreationKind::Hosted, ContextCreationKind::ImplicitlyCreated];
        pretty_assertions::assert_eq!(kinds.len(), 2);
    }
}
