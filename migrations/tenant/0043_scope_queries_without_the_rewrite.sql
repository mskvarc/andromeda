-- ============================================================================
-- 0043: Scope queries without the rewrite
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.19
--
-- Every branch, the live one and each Snapshot's, has rewritten the
-- `scope_q` of its Subscriptions, so nothing calls a function of the
-- `scope_query_rewrite` schema again. They are dropped by name, and the
-- schema after them without `cascade`, so a function left out of the list
-- fails the migration rather than outlive the rewrite.
-- ============================================================================
drop function scope_query_rewrite.scope_query_without_combined_any_scope, scope_query_rewrite.holds_combined_any_scope;

drop schema scope_query_rewrite;
