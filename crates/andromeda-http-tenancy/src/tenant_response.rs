//! Turning a tenant's domain view into the fixed-JSON `Tenant` resource.

use crate::{error::TenancyHttpError, options::TenantOptions};
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, uri::Uri};
use andromeda_ngsi_ld_model::tenant::{Tenant, TenantState, TenantUsage, tenant_id, tenant_name_from_id};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::cursor::PageCursor;
use andromeda_tenancy::{
    residency::TenantResidency,
    status::TenantStatus,
    tenant_keyset::TenantKeyset,
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

/// Map an incoming pagination cursor to the tenant keyset it addresses.
///
/// # Errors
///
/// Returns an `InvalidRequest` report when a forward or backward cursor's
/// boundary identifier does not name a tenant.
pub fn keyset_from_cursor(cursor: Option<&PageCursor>) -> Result<TenantKeyset, ClassifiedReport> {
    match cursor {
        None => Ok(TenantKeyset::first_page()),
        Some(PageCursor::After(id)) => Ok(TenantKeyset::Forward {
            after: Some(name_from_boundary(id)?),
        }),
        Some(PageCursor::Before(id)) => Ok(TenantKeyset::Backward {
            before: name_from_boundary(id)?,
        }),
        Some(PageCursor::End) => Ok(TenantKeyset::FromEnd),
    }
}

/// The tenant name a cursor boundary identifier carries, or an `InvalidRequest`.
fn name_from_boundary(id: &Uri) -> Result<TenantName, ClassifiedReport> {
    tenant_name_from_id(id).ok_or_else(|| {
        ClassifiedReport::public(
            NgsiLdErrorType::InvalidRequest,
            format!("the pagination cursor does not address a tenant: {id}"),
        )
    })
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
mod tests {
    use crate::tenant_response::wire_state;
    use andromeda_ngsi_ld_model::tenant::TenantState;
    use andromeda_tenancy::{residency::TenantResidency, status::TenantStatus};

    #[test]
    fn the_wire_status_folds_admin_state_and_residency_totally() {
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Suspended, TenantResidency::Resident), TenantState::Suspended);
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Suspended, TenantResidency::NotResident), TenantState::Suspended);
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Active, TenantResidency::Resident), TenantState::Active);
        pretty_assertions::assert_eq!(wire_state(TenantStatus::Active, TenantResidency::NotResident), TenantState::Dormant);
    }
}
