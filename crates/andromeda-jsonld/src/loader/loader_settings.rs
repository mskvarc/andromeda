//! The deployment settings one context loader is built with.

use crate::{engine::options::RemoteContextFetch, loader::context_lifetimes::ContextLifetimes};
use std::time::Duration;
use typed_builder::TypedBuilder;

/// How a [`crate::loader::http::HttpContextLoader`] reaches documents and how
/// long it keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TypedBuilder)]
pub(crate) struct LoaderSettings {
    /// Whether configuration permits a remote fetch at all.
    pub(crate) allow_remote: RemoteContextFetch,
    /// How long one remote fetch may take.
    pub(crate) timeout: Duration,
    /// How many redirects one remote fetch may follow.
    pub(crate) max_redirects: u32,
    /// The entry bound of the in-process cache.
    pub(crate) cache_capacity: u64,
    /// The lifetimes assigned where no response states one.
    pub(crate) lifetimes: ContextLifetimes,
    /// The size beyond which a remote document is refused.
    pub(crate) max_context_bytes: usize,
}
