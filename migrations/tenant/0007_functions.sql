-- ============================================================================
-- 0007: Geometry-to-geography cast
-- ============================================================================
-- `geom_to_geog` wraps the `geometry` to `geography` cast so a stored
-- generated column can use it. PostGIS marks the cast `STABLE` because it
-- reads `spatial_ref_sys`, but for SRID 4326, the only SRID the schema
-- admits, the result is deterministic, so the wrapper is declared
-- `IMMUTABLE`.
--
-- The function lives in `public`, and a Snapshot schema reaches it through
-- its search path, so one definition serves every branch.
-- ============================================================================
create or replace function geom_to_geog (g geometry)
    returns geography
    language sql
    immutable strict parallel safe
    as $$
    select
        g::geography
$$;

