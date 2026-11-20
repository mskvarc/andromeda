-- ============================================================================
-- 0038: Context source subscription dataset ids
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 5.12, Table 5.2.12-1
--
-- A Context Source Registration Subscription keeps the `datasetId` its client
-- wrote, which clause 5.12 matches against the `datasetId` of each
-- registration, so a csource row stores `dataset_ids` as an entity row does.
-- The check on csource rows is added again without `dataset_ids`, and still
-- holds `local_only` at its default. Every row the check it replaces admitted
-- meets it, so it is added `not valid` and no row is read to add it.
--
-- Codes:
--
--   subscriptions.kind: 1 entity, 2 csource
-- ============================================================================
alter table subscriptions
    drop constraint chk_subscriptions_csource_defaults,
    add constraint chk_subscriptions_csource_defaults check (kind = 1 or not local_only) not valid;
