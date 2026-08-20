-- ============================================================================
-- 0022: Optional physical relation diagnostics
-- ============================================================================
-- pgstattuple is a PostgreSQL supplied module, but managed installations may
-- omit its control file. Install it when available and let the opt-in
-- controller report a per-database capability deferral otherwise. This
-- migration never grants execution or MAINTAIN privileges to the broker role;
-- those remain explicit operator decisions.
-- ============================================================================

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'pgstattuple') THEN
        CREATE EXTENSION IF NOT EXISTS pgstattuple;
    END IF;
END
$$;
