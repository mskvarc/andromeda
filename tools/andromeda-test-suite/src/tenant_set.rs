//! A set of tenants of one test, each brought into being by its first write (clause 5.5.10) and holding one Device,
//! and the requests a load sends to one of them: a read of the Device, an update of it, a batch upsert beside it.
//!
//! A broker bringing up many tenants at once answers a first write 503 while its tenant builds queue past its
//! admission deadline; the write is then sent again until the delivery deadline, and only the answer that is not a 503
//! is checked.

use crate::{
    broker::BrokerClient,
    fresh_tenant_writes::create_until_admitted,
    load_outcome::LoadOutcome,
    namespace::{Namespace, NamespaceError},
};
use futures::{StreamExt, stream};
use http::StatusCode;
use ngsi_ld::model::{BuildError, Entity, EntityFragment, EntityId, Input, Tenant, TenantError};
use std::{fmt, iter};

/// How many entities a batch upsert of [`TenantSet::upsert_batch`] writes.
const BATCH_SIZE: i16 = 8;

/// Why a tenant set could not be brought up.
#[derive(Debug, thiserror::Error)]
pub enum TenantSetError {
    /// A set of no tenant.
    #[error("a tenant set holds at least one tenant")]
    Empty,
    /// A name of the set is not valid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// A tenant name of the set is not a tenant name.
    #[error("a tenant name of the set is not valid")]
    Tenant(#[from] TenantError),
    /// The Device or a write of it could not be built.
    #[error("the Device of the set could not be built")]
    Build(#[from] BuildError),
    /// Some first writes did not create their tenant.
    #[error(transparent)]
    Incomplete(#[from] ProvisioningIncomplete),
}

/// The first writes of a set that did not answer 201, each with how it resolved.
#[derive(Debug, thiserror::Error)]
pub struct ProvisioningIncomplete {
    /// The tenants whose first write failed, and how it resolved.
    pub failures: Vec<(Tenant, LoadOutcome)>,
    /// How many tenants the set was to hold.
    pub total: usize,
}

// Hand-written: the failures read as one line of a failure message.
impl fmt::Display for ProvisioningIncomplete {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} of {} first writes did not answer 201: {:?}",
            self.failures.len(),
            self.total,
            self.failures
        )
    }
}

/// One tenant of a set.
#[derive(Debug, Clone)]
pub struct TenantMember {
    /// The tenant.
    pub tenant: Tenant,
    /// A client of the broker in the tenant.
    pub client: BrokerClient,
}

/// Tenants of one test, each holding the Device its first write created.
#[derive(Debug)]
pub struct TenantSet {
    /// The first tenant.
    head: TenantMember,
    /// Every other tenant, in order.
    tail: Vec<TenantMember>,
    /// The Device each tenant holds.
    device: EntityId,
    /// The update [`TenantSet::update`] sends.
    update: EntityFragment,
    /// The entities [`TenantSet::upsert_batch`] writes.
    batch: Vec<Entity<Input>>,
}

impl TenantSet {
    /// Brings up `count` tenants named `<namespace>-<n>` through `broker`, at most `concurrency` first writes at once,
    /// each writing the set's Device.
    ///
    /// # Errors
    ///
    /// Returns [`TenantSetError::Incomplete`] naming every first write that did not answer 201, and another
    /// [`TenantSetError`] when a value of the set cannot be built.
    pub async fn provision(broker: &BrokerClient, namespace: &Namespace, count: usize, concurrency: usize) -> Result<TenantSet, TenantSetError> {
        let device_type = namespace.entity_type("Device")?;
        let device_id = namespace.urn("Device", "member")?;
        let device = Entity::builder()
            .id(device_id.clone()) // clone: the set reads the Device by the id it is built with
            .entity_type(device_type.clone()) // clone: the batch entities share the type
            .property("reading", 1)
            .build()?;
        let members = (0..count)
            .map(|index| {
                let tenant = namespace.numbered_tenant(index)?;
                Ok(TenantMember {
                    client: broker.with_tenant(tenant.clone()), // clone: the member keeps the name its client is in
                    tenant,
                })
            })
            .collect::<Result<Vec<_>, TenantSetError>>()?;
        let outcomes = stream::iter(members.iter().map(|member| create_until_admitted(&member.client, &device)))
            .buffered(concurrency.max(1))
            .collect::<Vec<_>>()
            .await;
        let failures = members
            .iter()
            .zip(outcomes)
            .filter(|(_, outcome)| outcome.status() != Some(StatusCode::CREATED))
            .map(|(member, outcome)| (member.tenant.clone(), outcome)) // clone: the failure names the tenant the set keeps
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return Err(ProvisioningIncomplete { failures, total: count }.into());
        }
        let batch = (0_i16..BATCH_SIZE)
            .map(|index| {
                Ok(Entity::builder()
                    .id(namespace.urn("Device", format!("batch-{index}"))?)
                    .entity_type(device_type.clone()) // clone: every entity of the batch carries the type
                    .property("reading", index)
                    .build()?)
            })
            .collect::<Result<Vec<_>, TenantSetError>>()?;
        let mut members = members.into_iter();
        let head = members.next().ok_or(TenantSetError::Empty)?;
        Ok(TenantSet {
            head,
            tail: members.collect(),
            device: device_id,
            update: EntityFragment::builder().property("reading", 2).build()?,
            batch,
        })
    }

    /// How many tenants the set holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.tail.len() + 1
    }

    /// Whether the set holds no tenant, which it never does.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The tenant at `index`, counted from 0 and wrapping around the set.
    #[must_use]
    pub fn member(&self, index: usize) -> &TenantMember {
        match index % self.len() {
            0 => &self.head,
            position => self.tail.get(position - 1).unwrap_or(&self.head),
        }
    }

    /// Every tenant of the set, in order.
    pub fn members(&self) -> impl Iterator<Item = &TenantMember> {
        iter::once(&self.head).chain(&self.tail)
    }

    /// The Device each tenant holds.
    #[must_use]
    pub const fn device(&self) -> &EntityId {
        &self.device
    }

    /// Retrieves the Device of the tenant at `index`.
    pub async fn read(&self, index: usize) -> LoadOutcome {
        LoadOutcome::of(&self.member(index).client.with_responses().entities().retrieve(&self.device).send().await)
    }

    /// Updates the `reading` of the Device of the tenant at `index`.
    pub async fn update(&self, index: usize) -> LoadOutcome {
        let client = &self.member(index).client;
        LoadOutcome::of(&client.with_responses().entities().update_attributes(&self.device, &self.update).send().await)
    }

    /// Upserts a batch of entities beside the Device of the tenant at `index`.
    pub async fn upsert_batch(&self, index: usize) -> LoadOutcome {
        LoadOutcome::of(&self.member(index).client.with_responses().entities().batch_upsert(&self.batch).send().await)
    }
}
