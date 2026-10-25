-- ============================================================================
-- 0008: Functions every branch resolves
-- ============================================================================
--
-- The functions the branch relations name in a generated column and the
-- trigger function the dictionaries refuse changes with. They live in
-- `public`, and a Snapshot schema reaches them through its search path, so one
-- definition serves every branch.
-- ============================================================================
-- `geom_to_geog` wraps the `geometry` to `geography` cast so a stored
-- generated column can use it. PostGIS marks the cast `STABLE` because it
-- reads `spatial_ref_sys`, but for SRID 4326, the only SRID the schema
-- admits, the result is deterministic, so the wrapper is declared `IMMUTABLE`.
create or replace function geom_to_geog (g geometry)
    returns geography
    language sql
    immutable strict parallel safe
    as $$
    select
        g::geography
$$;

-- Refuses every change to a dictionary but the ones its triggers let through.
-- Keys of the dictionaries are held where no foreign key reaches, so they are
-- correct only while a key, once minted, names the same value for the life of
-- the tenant. Serves the row triggers and the statement triggers alike.
create or replace function refuse_dictionary_change ()
    returns trigger
    language plpgsql
    as $$
begin
    raise restrict_violation
    using message = tg_table_name || ' is append-only: a key once minted names the same value for the life of the tenant', table = tg_table_name;
end
$$;
