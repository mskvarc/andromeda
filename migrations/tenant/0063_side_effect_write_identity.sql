-- ============================================================================
-- 0063: Side-effect queue identity and write-scoped idempotency
-- ============================================================================
-- Two changes, one policy. `pending_side_effects.id` is now minted by the
-- broker as a version-7 UUID (`andromeda-context-commit` `SideEffectJobId`)
-- rather than defaulted by the database, so the enqueueing code knows the
-- identity of the row before its statement runs and re-runs that statement
-- with the same value. The default is dropped along with it: a row reaching
-- this table without an explicit identity is a writer bypassing the queue's
-- own identity discipline, and a column with no default rejects it.
--
-- A version-7 UUID encodes the instant it was minted. These identifiers are
-- internal -- the queue is never rendered through the API and no client ever
-- sees one -- so the exposure is to an operator reading the queue, who already
-- has `created_at` next to it. It is recorded as a decision rather than left
-- as a property nobody noticed.
--
-- `write_id` carries the identity of the atomic write that produced the job.
-- One commit merges every landed write's consequences into exactly one queued
-- job, so the column is unique and the enqueue is
-- `ON CONFLICT (write_id) DO NOTHING`: a commit replayed under its original
-- identity finds its job already queued and adds no second copy, which is what
-- keeps the at-least-once drain from executing one batch's side effects twice.
--
-- Drain-first policy, as in 0062. Rows enqueued before this migration carry no
-- write identity, and the write that produced them is gone, so no backfill can
-- reconstruct one: inventing values would either collide two unrelated rows
-- under one identity or hand every old row a private identity that dedupes
-- nothing. The queue must therefore be drained before this migration runs.
-- Adding a NOT NULL column with no default is itself that assertion: against a
-- non-empty table PostgreSQL rejects the statement and the migration fails
-- with the rows still queued, rather than silently admitting an unidentified
-- backlog.
--
-- The lease and dead-state invariants 0038 added are untouched: this migration
-- changes how a job is identified, not how it is claimed or retired.
-- ============================================================================

ALTER TABLE pending_side_effects ALTER COLUMN id DROP DEFAULT;

ALTER TABLE pending_side_effects ADD COLUMN write_id UUID NOT NULL;

CREATE UNIQUE INDEX idx_pse_write_identity ON pending_side_effects (write_id);
