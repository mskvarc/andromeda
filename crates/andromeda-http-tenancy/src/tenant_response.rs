//! Turning a tenant's domain view into the fixed-JSON `Tenant` resource.

use crate::{
    error::{CursorNotATenant, TenancyHttpError},
    options::TenantOptions,
};
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, tenant::TenantName};
use andromeda_ngsi_ld_model::tenant::{Tenant, TenantState, TenantUsage, tenant_id, tenant_name_from_id};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::{cursor::PageCursor, page_window::PageStart};
use andromeda_tenancy::{
    residency::TenantResidency,
    status::TenantStatus,
    tenant_page_start::TenantPageStart,
    tenant_view::TenantView,
    usage_probe::TenantUsageProbe,
};

/// Fold the persisted administrative state and the runtime residency into the
/// read-only wire status the resource reports.
///
/// Total over both inputs: a suspended tenant is `Suspended` regardless of
/// residency, and an active tenant is `Active` while resident and `Dormant` once
/// evicted.
#[must_use]
pub const fn wire_state(status: TenantStatus, residency: TenantResidency) -> TenantState {
    match status {
        TenantStatus::Suspended => TenantState::Suspended,
        TenantStatus::Active => match residency {
            TenantResidency::Resident => TenantState::Active,
            TenantResidency::NotResident => TenantState::Dormant,
        },
    }
}

/// The position in the tenant collection a page window starts at.
///
/// # Errors
///
/// Returns an `InvalidRequest` report when a forward or backward cursor's
/// boundary identifier does not name a tenant.
pub fn tenant_page_start(start: &PageStart) -> Result<TenantPageStart, ClassifiedReport> {
    match start {
        PageStart::Offset(offset) => Ok(TenantPageStart::Offset(*offset)),
        PageStart::Cursor(None) => Ok(TenantPageStart::Forward { after: None }),
        PageStart::Cursor(Some(PageCursor::After(id))) => Ok(TenantPageStart::Forward {
            after: Some(name_from_boundary(id)?),
        }),
        PageStart::Cursor(Some(PageCursor::Before(id))) => Ok(TenantPageStart::Backward {
            before: name_from_boundary(id)?,
        }),
        PageStart::Cursor(Some(PageCursor::End)) => Ok(TenantPageStart::FromEnd),
    }
}

/// The tenant name a cursor boundary identifier carries, or an `InvalidRequest`.
fn name_from_boundary(id: &Identifier) -> Result<TenantName, ClassifiedReport> {
    tenant_name_from_id(id).ok_or_else(|| ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, CursorNotATenant { id }.to_string()))
}

/// Build the fixed-JSON tenant resource for a view, gathering the opt-in usage
/// block only when `options` asks for it.
///
/// # Errors
///
/// Returns [`TenancyHttpError::TenantId`] when the resource identifier cannot be
/// built, and [`TenancyHttpError::Tenancy`] when the usage read fails.
pub async fn build_tenant(view: &TenantView, options: TenantOptions, usage_probe: &dyn TenantUsageProbe) -> Result<Tenant, TenancyHttpError> {
    let record = &view.record;
    let id = tenant_id(&record.tenant_name)?;
    let status = wire_state(record.status, view.residency);
    let (created_at, modified_at) = if options.sys_attrs {
        (Some(record.created_at), Some(record.modified_at))
    } else {
        (None, None)
    };
    let usage = if options.usage {
        let usage = usage_probe.usage(&record.database).await?;
        Some(TenantUsage {
            database_name: record.database.clone(), // clone: the resource owns the name the registry row keeps
            pool_connections: usage.pool_connections,
            database_size: usage.database_size,
            entity_count: usage.entity_count,
            snapshot_count: usage.snapshot_count,
            snapshot_quota: usage.snapshot_quota,
        })
    } else {
        None
    };
    Ok(Tenant::builder()
        .id(id)
        .tenant_name(record.tenant_name.clone()) // clone: the resource owns its name
        .is_active(record.status.is_active())
        .status(status)
        .created_at(created_at)
        .modified_at(modified_at)
        .usage(usage)
        .build())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::tenant_response::{tenant_page_start, wire_state};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_ngsi_ld_model::tenant::{TenantState, tenant_id};
    use andromeda_ngsi_ld_query::{cursor::PageCursor, page_window::PageStart};
    use andromeda_tenancy::{residency::TenantResidency, status::TenantStatus, tenant_page_start::TenantPageStart};

    fn name(raw: &str) -> TenantName {
        TenantName::try_from(raw.to_owned()).expect("a valid tenant name")
    }

    /// Clause 5.5.9.2: an offset window pages the collection by name offset.
    #[test]
    fn an_offset_window_becomes_an_offset_tenant_page() {
        pretty_assertions::assert_eq!(tenant_page_start(&PageStart::Offset(3)).ok(), Some(TenantPageStart::Offset(3)));
    }

    /// Clause 5.5.9.1: each cursor position addresses the tenant its boundary
    /// identifier names.
    #[test]
    fn a_cursor_window_becomes_a_keyset_tenant_page() {
        let acme = tenant_id(&name("acme")).expect("a tenant identifier");
        pretty_assertions::assert_eq!(tenant_page_start(&PageStart::Cursor(None)).ok(), Some(TenantPageStart::Forward { after: None }));
        pretty_assertions::assert_eq!(
            tenant_page_start(&PageStart::Cursor(Some(PageCursor::After(acme.clone())))).ok(), // clone: the identifier is reused below
            Some(TenantPageStart::Forward { after: Some(name("acme")) })
        );
        pretty_assertions::assert_eq!(
            tenant_page_start(&PageStart::Cursor(Some(PageCursor::Before(acme)))).ok(),
            Some(TenantPageStart::Backward { before: name("acme") })
        );
        pretty_assertions::assert_eq!(
            tenant_page_start(&PageStart::Cursor(Some(PageCursor::End))).ok(),
            Some(TenantPageStart::FromEnd)
        );
    }

    /// A cursor whose boundary names no tenant is refused.
    #[test]
    fn a_cursor_naming_no_tenant_is_refused() {
        let foreign = "urn:ngsi-ld:Building:1".parse().expect("valid URI");
        assert!(tenant_page_start(&PageStart::Cursor(Some(PageCursor::After(foreign)))).is_err());
    }

    #[test]
    fn the_wire_status_folds_admin_state_and_residency_totally() {
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Suspended, TenantResidency::Resident), TenantState::Suspended);
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Suspended, TenantResidency::NotResident), TenantState::Suspended);
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Active, TenantResidency::Resident), TenantState::Active);
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Active, TenantResidency::NotResident), TenantState::Dormant);
    }
}
