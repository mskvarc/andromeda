-- ============================================================================
-- 0031: Rowstore history chunks
-- ============================================================================
-- The `attribute_history` chunks the broker's conversion sweep keeps in the
-- rowstore, one row each, keyed by the chunk's start on the time dimension.
--
-- The columnstore segments each converted chunk by stream, and every segment
-- costs its batch metadata whatever it holds. A chunk holding few instances of
-- each stream, typically Entities written once and rarely changed, therefore
-- converts to more bytes than its heap and indexes take. The sweep counts a
-- chunk's rows and streams once it passes the conversion horizon and keeps it
-- in the rowstore when its rows per stream fall below the break-even of the
-- columnstore layout. A chunk past the horizon no longer receives appends, so
-- the verdict stands, and the count is not repeated on later sweeps.
--
-- Only the live branch holds the hypertable. A row whose chunk retention has
-- dropped is removed by the next sweep.
-- ============================================================================
create table rowstore_history_chunks (
    range_start timestamptz not null,
    row_count bigint not null,
    stream_count bigint not null,
    kept_at timestamptz not null,
    constraint pk_rowstore_history_chunks primary key (range_start),
    constraint chk_rowstore_history_chunks_row_count check (row_count >= 0),
    constraint chk_rowstore_history_chunks_stream_count check (stream_count >= 0 and stream_count <= row_count)
);

-- The rowstore chunks of `attribute_history` whose indexes the maintenance
-- controller rebuilt after the chunk closed, one row each, keyed by the
-- chunk's start on the time dimension. A chunk's indexes reach the density
-- of appends in write order, and a closed chunk receives no more appends, so
-- one rebuild packs them for good and the controller rebuilds each chunk
-- once. A row whose chunk retention has dropped is removed after the next
-- rebuild.
create table rebuilt_history_chunks (
    range_start timestamptz not null,
    rebuilt_at timestamptz not null,
    constraint pk_rebuilt_history_chunks primary key (range_start)
);
