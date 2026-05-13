#!/usr/bin/env python3
"""Phase 2 dataset generator.

Deterministic (fixed seed) generator for the issue-tracker workload
described in benchmarks/phase2/schema.edn. Emits two files per run:

  mentat_tx.sql   — SELECT mentat_transact('[{:db/id ... ...}]'); batches
  eav_load.sql    — INSERT INTO eav.<type> (e, a, v) VALUES ...; batches

Both files load exactly the same entities and produce the same datom
count, so mentat-vs-EAV comparisons are apples-to-apples.

Usage:
    python3 gen_dataset.py <n_users> <n_issues> <n_labels> <out_dir>

Example:
    python3 gen_dataset.py 1000 10000 50 /tmp/phase2-data/
"""

import os
import random
import sys
from datetime import datetime, timedelta, timezone

SEED = 20260509

# Attribute IDs — must match eav_baseline/schema.sql ATTR_IDS.
# pg_mentat assigns its own entids at schema-install time; for the EAV
# baseline we hard-code these so the loader knows where to put each value.
ATTR_EMAIL     = 1000
ATTR_NAME      = 1001
ATTR_TITLE     = 1002
ATTR_STATE     = 1003
ATTR_PRIORITY  = 1004
ATTR_ASSIGNEE  = 1005
ATTR_REPORTER  = 1006
ATTR_CREATED   = 1007
ATTR_LABEL     = 1008
ATTR_LABELNAME = 1009

STATES = [":state/open", ":state/in-progress", ":state/closed", ":state/resolved", ":state/reopened"]
PRIORITIES = [1, 2, 3, 4, 5]
BATCH = 2000


def gen_entity_ids(n_users, n_issues, n_labels):
    """Allocate stable entity IDs per entity class.

    User ids : 100_000 .. 100_000 + n_users
    Label ids: 200_000 .. 200_000 + n_labels
    Issue ids: 300_000 .. 300_000 + n_issues
    """
    return {
        "user":  list(range(100_000, 100_000 + n_users)),
        "label": list(range(200_000, 200_000 + n_labels)),
        "issue": list(range(300_000, 300_000 + n_issues)),
    }


def main():
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    n_users  = int(sys.argv[1])
    n_issues = int(sys.argv[2])
    n_labels = int(sys.argv[3])
    out_dir  = sys.argv[4]
    os.makedirs(out_dir, exist_ok=True)

    rng = random.Random(SEED)
    ids = gen_entity_ids(n_users, n_issues, n_labels)
    base_time = datetime(2025, 1, 1, tzinfo=timezone.utc)

    # Precompute per-entity payloads so both outputs stay consistent
    users = []
    for u in ids["user"]:
        users.append({
            "id":    u,
            "email": f"user{u}@example.com",
            "name":  f"User {u - 100_000}",
        })

    labels = []
    for l in ids["label"]:
        labels.append({"id": l, "name": f"label-{l - 200_000}"})

    issues = []
    for i in ids["issue"]:
        n_issue_labels = rng.randint(0, min(3, n_labels))
        issue_labels = rng.sample(ids["label"], n_issue_labels) if n_labels else []
        issues.append({
            "id":        i,
            "title":     f"Issue {i - 300_000}: " + rng.choice(["crash", "ui glitch", "perf regression", "typo", "feature request"]),
            "state":     rng.choice(STATES),
            "priority":  rng.choice(PRIORITIES),
            "assignee":  rng.choice(ids["user"]),
            "reporter":  rng.choice(ids["user"]),
            "created":   (base_time + timedelta(minutes=rng.randint(0, 525_600))).strftime("%Y-%m-%dT%H:%M:%S+00:00"),
            "labels":    issue_labels,
        })

    # -- Write mentat transaction file -------------------------------------
    mentat_path = os.path.join(out_dir, "mentat_tx.sql")
    with open(mentat_path, "w") as f:
        f.write("SET search_path = mentat, public;\n")
        entities = []
        for u in users:
            entities.append(
                f'{{:db/id {u["id"]} '
                f':user/email "{u["email"]}" '
                f':user/name "{u["name"]}"}}'
            )
        for l in labels:
            entities.append(
                f'{{:db/id {l["id"]} :label/name "{l["name"]}"}}'
            )
        for i in issues:
            # Main issue entity (cardinality-one attrs only). Labels are
            # cardinality-many refs; the map-form parser rejects a vector
            # value here, so they go out as separate [:db/add e attr v]
            # assertions after the issue entity is defined.
            entities.append(
                f'{{:db/id {i["id"]} '
                f':issue/title "{i["title"]}" '
                f':issue/state {i["state"]} '
                f':issue/priority {i["priority"]} '
                f':issue/assignee {i["assignee"]} '
                f':issue/reporter {i["reporter"]} '
                f':issue/created-at #inst "{i["created"]}"}}'
            )
            for lbl in i["labels"]:
                entities.append(f'[:db/add {i["id"]} :issue/label {lbl}]')
        # Batch emits
        for start in range(0, len(entities), BATCH):
            batch = entities[start:start + BATCH]
            edn = "[\n  " + "\n  ".join(batch) + "\n]"
            edn_sql = edn.replace("'", "''")
            f.write(f"SELECT mentat_transact('{edn_sql}');\n")

    # -- Write EAV load file -----------------------------------------------
    eav_path = os.path.join(out_dir, "eav_load.sql")
    with open(eav_path, "w") as f:
        f.write("-- EAV baseline: one INSERT .. VALUES per batch per type\n")
        f.write("SET search_path = eav, public;\n\n")

        # Collect (e, a, v) tuples per type
        long_rows    = []
        text_rows    = []
        kw_rows      = []
        ref_rows     = []
        instant_rows = []

        for u in users:
            text_rows.append((u["id"], ATTR_EMAIL, u["email"].replace("'", "''")))
            text_rows.append((u["id"], ATTR_NAME,  u["name"].replace("'", "''")))
        for l in labels:
            text_rows.append((l["id"], ATTR_LABELNAME, l["name"].replace("'", "''")))
        for i in issues:
            text_rows.append((i["id"], ATTR_TITLE,    i["title"].replace("'", "''")))
            kw_rows.append  ((i["id"], ATTR_STATE,    i["state"]))
            long_rows.append((i["id"], ATTR_PRIORITY, i["priority"]))
            ref_rows.append ((i["id"], ATTR_ASSIGNEE, i["assignee"]))
            ref_rows.append ((i["id"], ATTR_REPORTER, i["reporter"]))
            instant_rows.append((i["id"], ATTR_CREATED, i["created"]))
            for lbl in i["labels"]:
                ref_rows.append((i["id"], ATTR_LABEL, lbl))

        def emit(rows, table, value_fmt):
            for start in range(0, len(rows), BATCH):
                chunk = rows[start:start + BATCH]
                values = ",\n  ".join(f"({e}, {a}, {value_fmt.format(v=v)})" for e, a, v in chunk)
                f.write(f"INSERT INTO eav.{table} (e, a, v) VALUES\n  {values}\nON CONFLICT DO NOTHING;\n")

        emit(long_rows,    "long",    "{v}")
        emit(text_rows,    "text",    "'{v}'")
        emit(kw_rows,      "keyword", "'{v}'")
        emit(ref_rows,     "ref",     "{v}")
        emit(instant_rows, "instant", "'{v}'::TIMESTAMPTZ")

    # -- Summary -----------------------------------------------------------
    n_datoms = (
        len(users) * 2                          # email, name
      + len(labels)                             # name
      + len(issues) * 6                         # title, state, priority, assignee, reporter, created-at
      + sum(len(i["labels"]) for i in issues)   # label refs
    )
    meta_path = os.path.join(out_dir, "meta.txt")
    with open(meta_path, "w") as f:
        f.write(f"seed: {SEED}\n")
        f.write(f"n_users: {n_users}\n")
        f.write(f"n_issues: {n_issues}\n")
        f.write(f"n_labels: {n_labels}\n")
        f.write(f"n_datoms: {n_datoms}\n")
        f.write(f"mentat_tx_bytes: {os.path.getsize(mentat_path)}\n")
        f.write(f"eav_load_bytes: {os.path.getsize(eav_path)}\n")

    # Print summary to stdout
    print(f"gen_dataset: wrote {mentat_path} and {eav_path}")
    print(f"gen_dataset: users={n_users} issues={n_issues} labels={n_labels} total datoms={n_datoms}")


if __name__ == "__main__":
    main()
