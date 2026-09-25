//! The worker identities and lease terms the queue verifications claim under.

use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_notification::delivery_lease::LeaseTerm;

/// One notification delivery worker, distinct from every other.
pub fn delivery_worker() -> LeaseHolder {
    LeaseHolder::mint()
}

/// One side-effect worker, distinct from every other.
pub fn side_effect_worker() -> LeaseHolder {
    LeaseHolder::mint()
}

/// A lease long enough that it cannot expire during a verification.
pub fn holding_lease() -> LeaseTerm {
    LeaseTerm::from_seconds(30).expect("a lease term inside the accepted range")
}

/// The shortest lease the term type accepts, for reclaim verifications.
pub fn expiring_lease() -> LeaseTerm {
    LeaseTerm::from_seconds(1).expect("a lease term inside the accepted range")
}
