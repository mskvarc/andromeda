-- ============================================================================
-- 0008: Functions every branch resolves
-- ============================================================================
--
-- The trigger function the dictionaries refuse changes with. It lives in
-- `public`, and a Snapshot schema reaches it through its search path, so one
-- definition serves every branch.
-- ============================================================================
-- Refuses every change to a dictionary but the ones its triggers let through.
-- Keys of the dictionaries are held where no foreign key reaches, so they are
-- correct only while a key, once minted, names the same value for the life of
-- the tenant. Serves the row triggers and the statement triggers alike.
create function refuse_dictionary_change ()
    returns trigger
    language plpgsql
    as $$
begin
    raise restrict_violation
    using message = tg_table_name || ' is append-only: a key once minted names the same value for the life of the tenant', table = tg_table_name;
end
$$;
