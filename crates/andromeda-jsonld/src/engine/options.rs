//! The three choices a caller makes about a JSON-LD operation.

use strum::Display;

/// Whether a compacted document keeps its `@context`.
///
/// A response body that names its context needs it kept; a value that is
/// about to be embedded in a larger document does not, and clause 5.5.7
/// would otherwise have it declared twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Display)]
pub enum ContextStripping {
    /// Remove `@context` from the compacted output.
    #[default]
    Strip,
    /// Keep `@context` in the compacted output.
    Keep,
}

/// Whether the engine may fetch a context it does not already hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum RemoteContextFetch {
    /// Fetch contexts over HTTP when they are not embedded or cached.
    Allow,
    /// Refuse every remote fetch.
    Deny,
}

impl From<bool> for RemoteContextFetch {
    /// `true` is [`Allow`](RemoteContextFetch::Allow), `false` is
    /// [`Deny`](RemoteContextFetch::Deny).
    fn from(allow: bool) -> RemoteContextFetch {
        if allow { RemoteContextFetch::Allow } else { RemoteContextFetch::Deny }
    }
}

/// How one compaction is performed.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompactOptions {
    /// Whether the compacted document keeps its `@context`.
    pub strip_context: ContextStripping,
}

#[cfg(test)]
mod tests {
    use crate::engine::options::RemoteContextFetch;

    #[test]
    fn the_configuration_flag_maps_onto_the_two_fetch_policies() {
        pretty_assertions::assert_eq!(RemoteContextFetch::from(true), RemoteContextFetch::Allow);
        pretty_assertions::assert_eq!(RemoteContextFetch::from(false), RemoteContextFetch::Deny);
    }
}
