#!/usr/bin/env python3
"""Table census generator for the pftui data architecture (R0).

Enumerates every table in (a) a freshly migrated DB and (b) an existing
live DB, plus every table name that appears in a CREATE TABLE statement
anywhere under src/. For each table it reports:

  - rowcount (live DB)
  - last-write proxy: MAX of the first timestamp-ish column found
  - writers: src files containing INSERT/UPDATE/DELETE/REPLACE on the table
  - readers: src files containing FROM/JOIN on the table
  - created_by_code: whether any src file CREATEs the table
  - legacy: present in the live DB but not created by any code path

PRIVACY: this script reads ONLY metadata — table names, schemas, rowcounts,
and MAX() of timestamp columns. It never selects row contents. Point it at
a COPY of the live DB, never the real file.

Usage:
  python3 scripts/db_census.py --live-db <copy.db> --fresh-db <fresh.db> \
      --src src/ --out census.json
"""

import argparse
import json
import os
import re
import sqlite3
import sys

# Columns accepted as a "last write" proxy, in priority order.
TS_COLUMNS = [
    "updated_at", "created_at", "recorded_at", "fetched_at", "inserted_at",
    "observed_at", "scored_at", "snapshot_date", "as_of", "as_of_date",
    "last_updated", "timestamp", "ts", "date", "run_date", "entry_date",
]

# Require an opening paren after the name so prose in comments
# ("create table if needed") never matches.
CREATE_RE = re.compile(
    r"CREATE\s+(?:VIRTUAL\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?"
    r"([A-Za-z_][A-Za-z0-9_]*)\s*\(",
    re.IGNORECASE,
)

# Transient tables that exist only mid-migration (created, filled, renamed
# away inside one execute_batch). They never persist in any DB.
TRANSIENT_TABLES = {"calibration_matrix_canonical_rebuild"}

TEST_MARKERS = ("#[cfg(test)]", "\nmod tests {")


def rs_files(src_dir):
    for root, _dirs, files in os.walk(src_dir):
        for f in files:
            if f.endswith(".rs"):
                yield os.path.join(root, f)


def collect_source(src_dir):
    """Return {path: content} for all .rs files, truncated at the first
    test marker so test-only SQL never counts as a production writer/reader."""
    out = {}
    for path in rs_files(src_dir):
        with open(path, encoding="utf-8", errors="replace") as fh:
            content = fh.read()
        for marker in TEST_MARKERS:
            idx = content.find(marker)
            if idx != -1:
                content = content[:idx]
        out[path] = content
    return out


def code_created_tables(sources):
    created = {}
    for path, content in sources.items():
        for m in CREATE_RE.finditer(content):
            name = m.group(1)
            if name in TRANSIENT_TABLES:
                continue
            created.setdefault(name, set()).add(path)
    return created


def writer_reader_map(sources, tables, repo_root):
    """Grep-style detection of writers and readers per table."""
    writers = {t: set() for t in tables}
    readers = {t: set() for t in tables}
    w_res = {
        t: re.compile(
            r"(?:INSERT\s+(?:OR\s+\w+\s+)?INTO|REPLACE\s+INTO|UPDATE|DELETE\s+FROM)\s+"
            + re.escape(t) + r"\b",
            re.IGNORECASE,
        )
        for t in tables
    }
    r_res = {
        t: re.compile(r"(?:FROM|JOIN)\s+" + re.escape(t) + r"\b", re.IGNORECASE)
        for t in tables
    }
    for path, content in sources.items():
        rel = os.path.relpath(path, repo_root)
        for t in tables:
            if t not in content:
                continue
            if w_res[t].search(content):
                writers[t].add(rel)
            if r_res[t].search(content):
                readers[t].add(rel)
    return writers, readers


def db_tables(conn):
    rows = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' "
        "AND name NOT LIKE 'sqlite_%' ORDER BY name"
    ).fetchall()
    return [r[0] for r in rows]


def table_meta(conn, table):
    """Rowcount + last-write proxy. Metadata only — never row contents."""
    q = f'"{table}"'
    count = conn.execute(f"SELECT COUNT(*) FROM {q}").fetchone()[0]
    cols = [r[1] for r in conn.execute(f"PRAGMA table_info({q})").fetchall()]
    last_write, ts_col = None, None
    for cand in TS_COLUMNS:
        if cand in cols:
            val = conn.execute(f"SELECT MAX(\"{cand}\") FROM {q}").fetchone()[0]
            if val is not None:
                last_write, ts_col = str(val), cand
                break
    return count, last_write, ts_col


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--live-db", required=True, help="COPY of the live DB")
    ap.add_argument("--fresh-db", required=True, help="freshly migrated DB")
    ap.add_argument("--src", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    repo_root = os.path.dirname(os.path.abspath(args.src.rstrip("/")))
    sources = collect_source(args.src)
    created = code_created_tables(sources)

    live = sqlite3.connect(f"file:{args.live_db}?mode=ro", uri=True)
    fresh = sqlite3.connect(f"file:{args.fresh_db}?mode=ro", uri=True)
    live_tables = db_tables(live)
    fresh_tables = db_tables(fresh)

    all_tables = sorted(set(live_tables) | set(fresh_tables) | set(created))
    writers, readers = writer_reader_map(sources, all_tables, repo_root)

    census = {}
    for t in all_tables:
        in_live = t in live_tables
        rowcount, last_write, ts_col = (None, None, None)
        if in_live:
            rowcount, last_write, ts_col = table_meta(live, t)
        census[t] = {
            "in_live_db": in_live,
            "in_fresh_db": t in fresh_tables,
            "created_by_code": t in created,
            "create_sites": sorted(
                os.path.relpath(p, repo_root) for p in created.get(t, [])
            ),
            "rowcount": rowcount,
            "last_write": last_write,
            "last_write_column": ts_col,
            "writers": sorted(writers[t]),
            "readers": sorted(readers[t]),
            "legacy": in_live and t not in created,
        }

    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(census, fh, indent=2)

    n = len(census)
    legacy = sum(1 for v in census.values() if v["legacy"])
    dead = sum(
        1 for v in census.values()
        if v["created_by_code"] and not v["writers"] and not v["readers"]
    )
    print(f"census: {n} tables ({len(live_tables)} live, "
          f"{len(fresh_tables)} fresh, {len(created)} code-created); "
          f"{legacy} legacy, {dead} with no writers AND no readers",
          file=sys.stderr)


if __name__ == "__main__":
    main()
