//! The tenant resource the broker exposes at `GET /ngsi-ld/v1/tenants` and
//! `GET /ngsi-ld/v1/tenants/{tenantName}`.
//!
//! Tenant management is not defined by ETSI GS CIM 009 v1.9.1, which leaves it
//! implementation-specific (clause 5.5.10) beyond the `NGSILD-Tenant` header
//! (clause 6.3.14). This resource is therefore an Andromeda extension. It borrows
//! the clause 5.2.12 idiom the specification uses for a Subscription -- a
//! client-writable `isActive` Boolean paired with a read-only, system-generated
//! `status` -- and returns a fixed JSON shape (no JSON-LD expansion variants), so
//! the operational members carry plain vendor names rather than expandable
//! `ngsi-ld:` terms.

use andromeda_ngsi_ld_lexicon::{
    constant_string,
    identifier::{Identifier, IdentifierParseError},
    rfc3339,
    tenant::TenantName,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

constant_string!(TenantType, "Tenant");

/// The read-only lifecycle status a tenant resource reports.
///
/// A broker extension modelled on the clause 5.2.12 Subscription `status`: it
/// folds the persisted administrative state together with runtime residency, so
/// it conveys residency the client-writable `isActive` Boolean cannot. The
/// `dormant` value in particular is non-spec and is a documented vendor value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum TenantState {
    /// The tenant is in service and a runtime is resident.
    Active,
    /// The tenant is in service but no runtime is resident.
    Dormant,
    /// The tenant is administratively suspended and serves no data requests.
    Suspended,
}

/// The opt-in operational usage a tenant resource carries under `?options=usage`.
///
/// Every member is a broker-defined operational metric rather than an NGSI-LD
/// term. Because the tenant resource is returned as fixed JSON (never JSON-LD
/// expanded against an `@context`), these are plain camelCase members, not
/// vendor IRIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct TenantUsage {
    /// Live PostgreSQL backend connections serving the tenant's database.
    pub pool_connections: u64,
    /// Total on-disk size of the tenant's database, in bytes.
    pub database_size: u64,
    /// Number of current entities the tenant holds.
    pub entity_count: u64,
    /// Number of snapshots the tenant holds.
    pub snapshot_count: u64,
    /// The configured ceiling on the tenant's snapshot count, if one is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub snapshot_quota: Option<u64>,
}

/// A tenant resource.
///
/// Cheap-by-default: `id`, `type`, `tenantName`, `isActive` and `status` come from
/// the registry row and the in-memory runtime cache. The system timestamps appear
/// only under `?options=sysAttrs` (clause 6.3.11), and the operational
/// [`TenantUsage`] only under `?options=usage`; both default to absent, so a plain
/// read costs no tenant-database query.
#[derive(Debug, Clone, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Tenant {
    /// The tenant's identifier, `urn:ngsi-ld:Tenant:{tenantName}`.
    pub id: Identifier,
    /// The fixed `Tenant` type discriminator.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: TenantType,
    /// The tenant's name.
    pub tenant_name: TenantName,
    /// Whether the tenant is administratively in service (clause 5.2.12-1,
    /// client-writable Boolean).
    pub is_active: bool,
    /// The read-only, system-computed lifecycle status.
    pub status: TenantState,
    /// When the tenant was created (clause 4.8), present only under `sysAttrs`.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime")]
    #[builder(default = Default::default())]
    pub created_at: Option<DateTime<Utc>>,
    /// When the tenant was last modified (clause 4.8), present only under
    /// `sysAttrs`.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime")]
    #[builder(default = Default::default())]
    pub modified_at: Option<DateTime<Utc>>,
    /// The operational usage, present only under `usage`. Flattened so its
    /// members sit at the top level of the resource.
    #[serde(flatten)]
    #[builder(default = Default::default())]
    pub usage: Option<TenantUsage>,
}

/// The identifier a tenant of the given name is exposed under.
///
/// # Errors
///
/// Returns [`IdentifierParseError`] when the name cannot be assembled into an NGSI-LD
/// URN, which a validated [`TenantName`] never triggers in practice.
pub fn tenant_id(name: &TenantName) -> Result<Identifier, IdentifierParseError> {
    format!("urn:ngsi-ld:Tenant:{}", name.as_str()).parse::<Identifier>()
}

/// The tenant name a tenant identifier carries, if it is a well-formed tenant id.
///
/// The inverse of [`tenant_id`], used to turn a pagination cursor's boundary
/// identifier back into the name the registry orders by.
#[must_use]
pub fn tenant_name_from_id(id: &Identifier) -> Option<TenantName> {
    let rendered = id.to_string();
    let name = rendered.strip_prefix("urn:ngsi-ld:Tenant:")?;
    TenantName::try_from(name.to_string()).ok()
}

#[cfg(test)]
mod tests {
    use crate::tenant::{Tenant, TenantState, TenantUsage, tenant_id, tenant_name_from_id};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, tenant::TenantName};
    use chrono::{TimeZone, Utc};

    fn name(raw: &str) -> TenantName {
        TenantName::try_from(raw.to_string()).expect("a valid tenant name")
    }

    #[test]
    fn a_default_tenant_carries_only_the_cheap_members() {
        let tenant = Tenant::builder()
            .id(tenant_id(&name("acme")).expect("id"))
            .tenant_name(name("acme"))
            .is_active(true)
            .status(TenantState::Active)
            .build();
        let json = serde_json::to_value(&tenant).expect("serialises");
        pretty_assertions::assert_eq!(
            json,
            serde_json::json!({
                "id": "urn:ngsi-ld:Tenant:acme",
                "type": "Tenant",
                "tenantName": "acme",
                "isActive": true,
                "status": "active"
            })
        );
    }

    #[test]
    fn a_suspended_tenant_reports_is_active_false_and_status_suspended() {
        let tenant = Tenant::builder()
            .id(tenant_id(&name("acme")).expect("id"))
            .tenant_name(name("acme"))
            .is_active(false)
            .status(TenantState::Suspended)
            .build();
        let json = serde_json::to_value(&tenant).expect("serialises");
        pretty_assertions::assert_eq!(json["isActive"], serde_json::json!(false));
        pretty_assertions::assert_eq!(json["status"], serde_json::json!("suspended"));
    }

    #[test]
    fn sys_attrs_add_top_level_timestamps() {
        let created = Utc.with_ymd_and_hms(2026, 9, 21, 8, 0, 0).single().expect("instant");
        let modified = Utc.with_ymd_and_hms(2026, 9, 21, 9, 30, 0).single().expect("instant");
        let tenant = Tenant::builder()
            .id(tenant_id(&name("acme")).expect("id"))
            .tenant_name(name("acme"))
            .is_active(true)
            .status(TenantState::Dormant)
            .created_at(Some(created))
            .modified_at(Some(modified))
            .build();
        let json = serde_json::to_value(&tenant).expect("serialises");
        pretty_assertions::assert_eq!(json["createdAt"], serde_json::json!("2026-09-21T08:00:00Z"));
        pretty_assertions::assert_eq!(json["modifiedAt"], serde_json::json!("2026-09-21T09:30:00Z"));
    }

    #[test]
    fn usage_flattens_its_members_to_the_top_level() {
        let usage = TenantUsage::builder()
            .pool_connections(3)
            .database_size(4096)
            .entity_count(7)
            .snapshot_count(2)
            .snapshot_quota(Some(16))
            .build();
        let tenant = Tenant::builder()
            .id(tenant_id(&name("acme")).expect("id"))
            .tenant_name(name("acme"))
            .is_active(true)
            .status(TenantState::Active)
            .usage(Some(usage))
            .build();
        let json = serde_json::to_value(&tenant).expect("serialises");
        pretty_assertions::assert_eq!(json["databaseSize"], serde_json::json!(4096));
        pretty_assertions::assert_eq!(json["entityCount"], serde_json::json!(7));
        pretty_assertions::assert_eq!(json["poolConnections"], serde_json::json!(3));
        pretty_assertions::assert_eq!(json["snapshotCount"], serde_json::json!(2));
        pretty_assertions::assert_eq!(json["snapshotQuota"], serde_json::json!(16));
    }

    #[test]
    fn a_tenant_identifier_round_trips_through_its_name() {
        let id = tenant_id(&name("acme")).expect("id");
        pretty_assertions::assert_eq!(id.to_string(), "urn:ngsi-ld:Tenant:acme");
        pretty_assertions::assert_eq!(tenant_name_from_id(&id), Some(name("acme")));
    }

    #[test]
    fn a_non_tenant_identifier_yields_no_name() {
        let id = "urn:ngsi-ld:Entity:1".parse::<Identifier>().expect("uri");
        assert!(tenant_name_from_id(&id).is_none());
    }
}
