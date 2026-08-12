//! The worker identities and lease terms the queue verifications claim under.

use andromeda_entity::side_effect_queue::SideEffectLeaseOwner;
use andromeda_notification::{delivery_lease::LeaseTerm, worker_instance_id::WorkerInstanceId};
use uuid::Uuid;

/// One notification delivery worker, distinct from every other.
pub fn delivery_worker() -> WorkerInstanceId {
    WorkerInstanceId::delivery(Uuid::new_v4())
}

/// One side-effect worker, distinct from every other.
pub fn side_effect_worker() -> SideEffectLeaseOwner {
    let source = "urn:ngsi-ld:ContextSourceIdentity:durable-queue"
        .parse()
        .expect("a valid context source identity URI");
    SideEffectLeaseOwner::for_tenant(&source, Uuid::new_v4())
}

/// A lease long enough that it cannot expire during a verification.
pub fn holding_lease() -> LeaseTerm {
    LeaseTerm::from_seconds(30).expect("a lease term inside the accepted range")
}

/// The shortest lease the term type accepts, for reclaim verifications.
pub fn expiring_lease() -> LeaseTerm {
    LeaseTerm::from_seconds(1).expect("a lease term inside the accepted range")
}
