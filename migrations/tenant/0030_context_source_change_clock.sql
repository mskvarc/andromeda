-- ============================================================================
-- 0030: Context source change clock
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.11.2.4, 5.11.3.4
--
-- The order in which context source registrations and csource subscriptions
-- were written. Every create, update or deletion of a registration, every
-- expiry sweep over them, and every client write of a csource subscription
-- advances `revision` by one in its own transaction and keeps the revision it
-- drew. The row lock the advance takes is held until that transaction ends, so
-- no two such writes draw revisions in an order other than the one they
-- commit in, whichever broker sharing the database makes them.
--
-- A registration change is matched against a csource subscription only when
-- the subscription's latest write drew the lower revision; a subscription
-- written later is notified of the change by its own write.
-- ============================================================================
create table context_source_change_clock (
    revision bigint not null,
    singleton boolean not null default true,
    constraint pk_context_source_change_clock primary key (singleton),
    constraint chk_context_source_change_clock_singleton check (singleton),
    constraint chk_context_source_change_clock_revision_non_negative check (revision >= 0)
);

insert into context_source_change_clock (revision)
values (0);
