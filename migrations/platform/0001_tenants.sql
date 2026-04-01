-- ============================================================================
-- Main DB: Tenants Registry
-- ============================================================================
-- Tracks tenant -> database mapping. This lives in the main DB.
-- The unnamed default tenant is represented by is_unnamed_default=true
-- with name=NULL (never by name="default").
-- ============================================================================

CREATE TABLE IF NOT EXISTS tenants (
    id BIGSERIAL PRIMARY KEY,
    name TEXT UNIQUE,
    is_unnamed_default BOOLEAN NOT NULL DEFAULT FALSE,
    db_name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_tenants_unnamed_default
    ON tenants (is_unnamed_default)
    WHERE is_unnamed_default = TRUE;
