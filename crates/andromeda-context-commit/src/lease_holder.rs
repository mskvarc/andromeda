//! The identity a worker holds a durable queue lease under.

use derive_more::{Display, From};
use uuid::Uuid;

/// The identity one background worker records while it holds a lease on a
/// claimable row.
///
/// Every durable queue the broker drains -- notification deliveries, pending
/// side effects, temporal append intents, Snapshot status notifications and
/// Snapshot preparation -- claims its rows under the same kind of holder, and
/// every one of them stores it in a `claimed_by UUID` column. The holder names
/// a worker, nothing more: what the worker is working for (a tenant, a source)
/// is reported beside it in the worker's own tracing fields, never folded into
/// the stored identity.
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, PartialEq)]
#[display("{_0}")]
pub struct LeaseHolder(Uuid);

impl LeaseHolder {
    /// Mint a fresh holder for one worker.
    #[must_use]
    pub fn mint() -> LeaseHolder {
        LeaseHolder(Uuid::now_v7())
    }

    /// The UUID the `claimed_by` column stores.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::lease_holder::LeaseHolder;
    use pretty_assertions::{assert_eq, assert_ne};
    use uuid::Uuid;

    #[test]
    fn two_minted_holders_are_distinct() {
        assert_ne!(LeaseHolder::mint(), LeaseHolder::mint());
    }

    #[test]
    fn a_holder_stores_and_displays_as_its_uuid() {
        let holder = LeaseHolder::from(Uuid::nil());
        assert_eq!(holder.as_uuid(), Uuid::nil());
        assert_eq!(holder.to_string(), "00000000-0000-0000-0000-000000000000");
    }
}
