-- ============================================================================
-- 0007: Helper Functions
-- ============================================================================
-- Functions referenced by table definitions or application queries. Must
-- precede table migrations that use them (e.g. entity_search uses
-- geom_to_geog in a GENERATED ALWAYS AS column).
--
-- The function is tenant-global and lives in `public`. A branch migration
-- (`migrations/branch/`) run inside a Snapshot schema reaches it unqualified
-- through `search_path = snap_<key>, public`, so one definition serves the
-- live branch and every Snapshot branch.
-- ============================================================================

-- Immutable wrapper for geometry->geography cast.
-- Technically STABLE in PostGIS (depends on spatial_ref_sys), but deterministic
-- for SRID 4326. Marking IMMUTABLE enables GENERATED ALWAYS AS usage.
-- This is a well-established PostGIS pattern for stored generated columns.
CREATE OR REPLACE FUNCTION geom_to_geog(g geometry)
RETURNS geography
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE
AS $$ SELECT g::geography $$;
