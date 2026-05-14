# Transactional Event Stream via PgQue

`pg_mentat` integrates with [`PgQue`][pgque] (NikolayS/PgQue, Apache 2.0)
to emit one event per transaction into a durable Postgres-backed
event stream. Every successful `mentat.t` (or any other commit that
writes to `mentat.transactions`) produces a `mentat.tx`-typed PgQue
event whose `ev_data` is the full datom payload of that tx.

[pgque]: https://github.com/NikolayS/PgQue

PgQue is the modern, managed-Postgres-friendly revival of Skype's
PgQ engine (~2007). It uses snapshot-based batching and TRUNCATE
table rotation — zero dead-tuple bloat under sustained load — and
ships as a single SQL file with no C extension, no
`shared_preload_libraries`, no external daemon. Works on any PG14+,
including RDS / Aurora / Cloud SQL / AlloyDB / Supabase / Neon.

`PgQue` is an **optional** dependency. Detect with
`mentat.has_pgque()`. The integration installs nothing on its own —
you opt in per queue with `mentat.pgque_emit_tx('queue_name')`.

## When to use this

| Use case | Pattern |
|:---|:---|
| Audit log / compliance trail | One event per tx; archive consumer drains to S3/Glacier. |
| Cache invalidation | Subscribe to events, invalidate downstream caches by attribute or entity. |
| Search-index update | Reindex affected entities into Elastic / Meilisearch / Tantivy on each event. |
| Webhook fan-out | One consumer maps events to outbound HTTP per matching pattern. |
| Replication to non-pg_mentat consumers | A consumer in a different language pulls events via PgQue's SQL API. |

It is **not** a replacement for logical replication. PgQue events
contain the EAV-shaped tx payload, not WAL records. For physical /
logical replication of the storage tables, use Postgres replication.

## Quick start

```bash
# Install PgQue (one-time per database).
git clone https://github.com/NikolayS/PgQue
cd PgQue
psql -d mydb -f sql/pgque.sql
```

```sql
CREATE EXTENSION pg_mentat;

-- Enable per-tx emit on a queue (creates the queue if missing,
-- attaches a deferred constraint trigger to mentat.transactions).
SELECT mentat.pgque_emit_tx('mentat_events');

-- Register a consumer that will receive events.
SELECT mentat.pgque_register_consumer('mentat_events', 'reporting');

-- Now every mentat.t produces an event at COMMIT time.
SELECT mentat.t('[
  {:db/ident :p/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
]');
SELECT mentat.t('[{:db/id "a" :p/name "Alice"}]');

-- Drive the queue (or use pg_cron / pg_timetable to do it for you).
SELECT pgque.force_next_tick('mentat_events');

-- Pull a batch.
DO $$
DECLARE bid BIGINT := pgque.next_batch('mentat_events', 'reporting');
BEGIN
  IF bid IS NOT NULL THEN
    -- Consumer code here.
    PERFORM pgque.finish_batch(bid);
  END IF;
END;
$$;
```

## Event shape (`ev_type = 'mentat.tx'`)

The `ev_data` field is a JSON envelope:

```json
{
  "tx": 1000007,
  "tx_instant": "2026-05-14T10:15:25.7+00:00",
  "store_id": "0",
  "datom_count": 4,
  "datoms": [
    {"e": 1000007, "a": 50, "v": "2026-05-14 10:15:25.7+00", "vt": "instant", "tx": 1000007, "added": true},
    {"e": 10001,   "a": 10000, "v": "Alice",                  "vt": "string",  "tx": 1000007, "added": true}
  ]
}
```

Field reference:

| Field | Type | Description |
|:---|:---|:---|
| `tx` | bigint | Transaction id from `mentat.partition_tx_seq`. |
| `tx_instant` | timestamptz | When the tx record was inserted. ISO-8601. |
| `store_id` | text | The `mentat.current_store_id` GUC at emit time. |
| `datom_count` | int | Number of datoms in this tx (rows of `datoms`). |
| `datoms` | array | One entry per added/retracted datom. |
| `datoms[].e` | bigint | Entity id. |
| `datoms[].a` | bigint | Attribute id. |
| `datoms[].v` | string | Value as text. Bytea values are hex-encoded. |
| `datoms[].vt` | string | Value type tag: `string`, `keyword`, `long`, `ref`, `double`, `boolean`, `instant`, `uuid`, `bytes`. |
| `datoms[].tx` | bigint | Same as outer `tx`. |
| `datoms[].added` | bool | `true` for assertion, `false` for retraction. |

## Why a deferred constraint trigger

`mentat.transactions` rows are inserted **early** in each tx, by
`mentat.current_tx()`, **before** the datoms for that tx are written
to the typed tables. A normal `AFTER INSERT FOR EACH ROW` trigger
would fire too early: the datom rows wouldn't exist yet.

The integration uses
`CREATE CONSTRAINT TRIGGER ... DEFERRABLE INITIALLY DEFERRED`. PostgreSQL
fires deferred constraint triggers at the **end** of the transaction,
right before COMMIT, by which point all datom inserts for the tx are
visible. The trigger function aggregates them across the 9 typed
tables, builds the JSON envelope, and calls `pgque.insert_event`.

If `pgque.insert_event` fails (e.g. someone dropped the schema), the
trigger swallows the exception and emits a `NOTICE` so the user's tx
still commits. This is a deliberate trade-off: queue emit failures
should **not** corrupt the transactional database.

## Multiple queues

You can emit to as many queues as you like — each `pgque_emit_tx`
call attaches its own deferred trigger:

```sql
SELECT mentat.pgque_emit_tx('audit_log');
SELECT mentat.pgque_emit_tx('search_index_updates');
SELECT mentat.pgque_emit_tx('webhooks');
-- Now every tx fires three triggers, one per queue.
```

The triggers fire in name order. Each one builds the same payload
(no work-sharing today). For high-volume workloads, prefer one queue
+ a fan-out consumer pattern instead of N queues — the trigger
overhead is per-queue.

## Disabling emit

```sql
SELECT mentat.pgque_disable_tx('audit_log');
-- => true if a trigger existed and was dropped, false otherwise
```

The PgQue queue itself is **not** dropped — it may still hold events
that haven't been consumed. To drain and remove:

```sql
-- Drain manually first, or just drop:
SELECT pgque.drop_queue('audit_log', force => true);
```

## Driving the ticker

PgQue's batching needs a ticker — something that periodically advances
the snapshot boundary. Options, in increasing order of latency
guarantee:

1. **`pg_cron` (recommended for managed Postgres)** —
   `SELECT pgque.start();` schedules a 1-second pg_cron slot that
   re-ticks every 100 ms internally. ~50 ms median end-to-end.
2. **`pg_timetable`** —
   `SELECT pgque.start_timetable();` for clusters running the
   external pg_timetable worker.
3. **External cron / systemd timer** — call `SELECT pgque.ticker();`
   on whatever cadence suits you.
4. **Manual ticking from your application** — useful in tests:
   `SELECT pgque.force_next_tick('audit_log');`

Tune cadence with `SELECT pgque.set_tick_period_ms(50);` (20 ticks/sec).
Allowed periods are exact divisors of 1000ms in `[1, 1000]`.

## Performance notes

- **Emit cost**: one `INSERT INTO pgque.event_<qid>` per pg_mentat
  tx, plus the JSON aggregation across the 9 typed tables for that
  tx's datoms. For most workloads this is < 1 ms.
- **Trigger ordering**: PostgreSQL fires constraint triggers in
  alphabetical name order. The per-queue trigger names are
  `mentat_pgque_emit_<sanitized-queue-name>`, so emits to multiple
  queues fire in queue-name order.
- **Bloat**: PgQue's TRUNCATE rotation means the event tables don't
  accumulate dead tuples regardless of throughput. The trigger on
  `mentat.transactions` does NOT cause additional bloat there
  either — it's an INSERT-only side effect.
- **Concurrent commits**: each tx runs the deferred trigger in its
  own commit phase. There's no global lock; emit throughput scales
  with commit throughput.

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `:db.error/missing-extension PgQue is not installed in this database` | Calling `pgque_emit_tx` before installing PgQue. | `\i sql/pgque.sql` from the PgQue source. |
| `mentat: pgque emit for queue X tx Y failed: ...` (NOTICE, not ERROR) | PgQue's queue or schema was modified after the trigger was installed. | The user tx still commits; queue emit is best-effort. Disable + re-enable the trigger after fixing the queue. |
| Consumer sees 0 events even though events were emitted | Consumer registered after the events were ticked into the current snapshot. | Register the consumer first; or call `pgque.force_next_tick` between events and consumer reads. |

## What this does NOT give you

- **Retroactive events.** Only transactions committed AFTER you call
  `pgque_emit_tx` produce events. Existing data is not backfilled.
- **Schema changes as separate events.** A schema-installing tx
  produces one combined event with the schema datoms; consumers must
  inspect `vt` / `a` to detect schema-affecting changes.
- **Causal ordering across stores.** Each store gets its own emit
  trigger with its own queue (or shares a queue but distinguishes
  via `store_id` in the payload). Cross-store causality is
  application-side.
- **Exactly-once delivery.** PgQue is at-least-once: a consumer that
  crashes between `next_batch` and `finish_batch` re-receives the
  same batch on next read. Make consumers idempotent.

## See also

- [PgQue README](https://github.com/NikolayS/PgQue) — full API,
  benchmarks, comparison with PGMQ / River / pg-boss / Que.
- [PostgreSQL deferred constraint triggers][dct] — the timing
  mechanism this integration relies on.
- The Skype PgQ paper, [PGCon 2009][pgq-paper] — original
  architecture.

[dct]: https://www.postgresql.org/docs/current/sql-createtrigger.html
[pgq-paper]: https://www.pgcon.org/2009/schedule/attachments/91_pgq.pdf
