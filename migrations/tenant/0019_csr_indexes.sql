-- ============================================================================
-- 0019: Partial indexes for context_source_registrations (C.3)
-- ============================================================================
-- CSR lookups during forwarding filter by status='ok' (healthy registrations
-- only -- failed ones are excluded until they recover). Without a partial
-- index, every forward must scan the whole table. A partial btree restricted
-- to `status = 'ok'` keeps the hot-path index small and omits the bloat from
-- failed registrations.
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_csr_status_active
    ON context_source_registrations (status)
    WHERE status = 'ok';
