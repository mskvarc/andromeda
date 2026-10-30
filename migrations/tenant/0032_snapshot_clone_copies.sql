-- ============================================================================
-- 0032: Snapshot clone copies
-- ============================================================================
-- The database session copying a clone's source into its staging branch, one
-- row per clone whose copy has started, keyed by the clone's Snapshot.
--
-- The copy holds the control records of the clone and of its source
-- `for key share` for as long as the source is large, and every path that
-- deletes a Snapshot takes `for update` on its control record first. A
-- deletion of either Snapshot reads this row and ends that session before it
-- locks, so it waits for the session to close rather than for the copy to
-- finish. A session is named by its process id and the start of the copy's
-- transaction together: the pooled session outlives the copy, and the
-- transaction start tells the copy apart from any later transaction of that
-- session. A row whose copy has ended names no running transaction and is
-- harmless; the publication of the clone removes it, and so does the deletion
-- of the Snapshot.
-- ============================================================================
create table snapshot_clone_copies (
    snapshot_key bigint not null,
    backend_pid bigint not null,
    transaction_started_at timestamptz not null,
    constraint pk_snapshot_clone_copies primary key (snapshot_key),
    constraint fk_snapshot_clone_copies_snapshot foreign key (snapshot_key) references snapshots (snapshot_key) on delete cascade,
    constraint chk_snapshot_clone_copies_backend_pid check (backend_pid > 0)
);
