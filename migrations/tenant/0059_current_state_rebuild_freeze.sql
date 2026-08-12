-- no-transaction
-- ============================================================================
-- 0059: Freeze and analyse the rebuilt current-state relations
-- ============================================================================
-- Both relations were bulk-loaded inside single transactions, so every tuple
-- carries a live xmin: without a freeze the first anti-wraparound vacuum
-- rewrites the whole of `entities`, payload TOAST included, at an arbitrary
-- later moment under load. The ANALYZE is equally load-bearing -- neither
-- relation has any statistics, the two extended statistics objects created in
-- 0057 have never been computed, and the first `q` query would otherwise plan
-- against defaults.
--
-- VACUUM cannot run inside a transaction block, so this file is marked
-- non-transactional and holds exactly one statement. VACUUM accepts a relation
-- list, which is what keeps the freeze to that one statement rather than two
-- files.
-- ============================================================================

VACUUM (FREEZE, ANALYZE) entities, current_attribute_values;
