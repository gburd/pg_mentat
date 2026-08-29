//! Datomic-in-Clojure-style scripting layer for pg_mentat, backed by the live
//! Postgres-resident store.
//!
//! This module embeds the pure-Rust [`mino_rs`] interpreter and registers the
//! `mentat.store/*` primitives as native host closures. Unlike the standalone
//! `mentat` crate — which owns a `rusqlite::Connection` — pg_mentat's store *is*
//! the current database, reached through the extension's own `#[pg_extern]`
//! engine functions (`transact`, `query`, `pull`, `entity`). A prim holds no
//! connection; it calls the engine at invocation time. The interpreter is a
//! plain Rust-heap value built, run, and dropped within one `mentat_eval` call.
//!
//! # The Datomic-a-like model
//!
//! Rich Hickey's crux: a *database value* is an immutable basis, not a mutable
//! connection.
//!
//!   * **conn handle** — an opaque `Int` from `mentat.store/open`. There is
//!     exactly one database (the one you are connected to), so `open` returns a
//!     trivial marker `1`. Mutating ops (`transact`) take a conn.
//!   * **db value** — an immutable snapshot, a mino map
//!     ```clojure
//!     {:mentat.store/db true,      ; tag
//!      :mentat.store/basis-tx T,   ; the basis tx id
//!      :mentat.store/as-of A,      ; nil, or an as-of point-in-time tx
//!      :mentat.store/since S}      ; nil, or a since floor tx
//!     ```
//!     `db` turns the current basis into a db value; `as-of`/`since`/`with`
//!     return *new* db values. All reads (`q`, `pull`, `entity`, `read`,
//!     `entities`, `datoms`) take a db value (a bare conn `Int` is accepted and
//!     treated as the current db). Eids may be integers, ident keywords, or
//!     `[:attr val]` lookup-refs.
//!
//! # Temporal honesty — pg_mentat's advantage
//!
//! Unlike Mentat's SQLite algebrizer (which has NO as-of query rewrite),
//! pg_mentat's `mentat_query` already accepts `{"asOf": T}` / `{"since": T}`
//! temporal inputs. So arbitrary Datalog `q` runs faithfully against a
//! historical basis: an as-of/since db simply forwards its bound into the query
//! inputs. No degenerate "unsupported" error.
//!
//! # The ABI is EDN + JSON
//!
//! mino values print to EDN via `print_str`; that EDN is fed to the engine.
//! The engine returns JSON; this layer converts `serde_json::Value` back to
//! mino values, restoring keyword typing (a leading `:` becomes a keyword) and
//! nesting maps/vectors recursively (pull results are not collapsed).
//!
//! # Caveats (mino-rs v0.1.0)
//!
//!   * **Pull patterns with symbols must be quoted.** `*` is the multiply prim,
//!     not self-evaluating, so `(pull db e [*])` evaluates `*` and fails; write
//!     `(pull db e '[*])`. Keyword-only patterns (`[:a :b]`) need no quote.
//!   * **`#inst`/`#uuid` cannot be supplied *in* via eval.** mino's evaluator
//!     expands an `#inst` literal to a calendar map and cannot eval `#uuid`. To
//!     transact an instant/uuid, pass it as a string and let the engine coerce.
//!     Instants read *out* (e.g. `:db/txInstant` via `datoms`) are rendered by
//!     pg_mentat's own JSON and pass through unchanged.
//!
//! Gated behind the `script` feature; nothing here compiles for a default build.

use mino_rs::collections::map::{PMap, PSet};
use mino_rs::collections::vector::PVec;
use mino_rs::error::throw_str;
use mino_rs::printer::print_str;
use mino_rs::symbol::Symbol;
use mino_rs::{Gc, Throw, Value};

use pgrx::prelude::*;

use serde_json::Value as J;

/// A fresh scripting interpreter with the `mentat.store/*` prims registered.
///
/// Built per `mentat_eval` call. `Interpreter::new()` loads `core.clj`; if that
/// proves hot, cache in a `thread_local!` (a backend is one thread) — but the
/// prims capture no connection, so a per-call interpreter is correct and
/// leak-free (the GC arena drops when the call returns).
pub fn build_interpreter() -> mino_rs::Interpreter {
    let mut it = mino_rs::Interpreter::new();
    register_store_prims(&mut it);
    it
}

/// Evaluate a mino script and return its result as EDN (`pr-str`) text.
///
/// Mirrors `mentat_transact`/`mentat_query`: EDN/text in, EDN text out. A mino
/// exception surfaces as a clean Postgres ERROR carrying the mino message, not
/// a panic backtrace.
#[pg_extern]
pub fn mentat_eval(script: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut it = build_interpreter();
    it.eval_to_string(script)
        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(format!("mentat_eval: {e}")))
}

// ---------------------------------------------------------------------------
// Prim registration
// ---------------------------------------------------------------------------

fn register_store_prims(it: &mut mino_rs::Interpreter) {
    // open: there is exactly one database. Return a trivial conn handle.
    it.register_prim_fn("mentat.store/open", |_it, _args| Ok(Value::Int(1)));

    // db: (db conn) -> a db value at the current basis (max committed tx).
    it.register_prim_fn("mentat.store/db", |_it, _args| {
        let basis = current_basis_tx().map_err(|e| throw_str(&e))?;
        Ok(db_value(basis, None, None))
    });

    // transact: (transact conn tx-data) -> the tx report as a map. Commits.
    it.register_prim_fn("mentat.store/transact", |_it, args| {
        let tx = args
            .get(1)
            .ok_or_else(|| throw_str("mentat.store/transact: expected (conn tx-data)"))?;
        let edn = print_str(tx);
        let report = super::transact::mentat_transact(&edn)
            .map_err(|e| throw_str(&format!("mentat.store/transact: {e}")))?;
        tx_report_value(&report)
    });

    // with: (with db tx-data) -> {:mentat.store/db-after db', :mentat.store/tx-report {...}}.
    // Speculative: runs the full pipeline in a SAVEPOINT and rolls back. No commit.
    it.register_prim_fn("mentat.store/with", |_it, args| {
        let db = destructure_db("mentat.store/with", args.first())?;
        let tx = args
            .get(1)
            .ok_or_else(|| throw_str("mentat.store/with: expected (db tx-data)"))?;
        if db.as_of.is_some() || db.since.is_some() {
            return Err(throw_str(
                "mentat.store/with: speculative transact is only defined against the current \
                 basis, not an as-of/since db.",
            ));
        }
        let edn = print_str(tx);
        let report = super::transact::mentat_with(&edn)
            .map_err(|e| throw_str(&format!("mentat.store/with: {e}")))?;
        let report_json = parse_json("mentat.store/with", &report)?;
        let basis_after = report_json
            .get("db-after")
            .and_then(|d| d.get("basis-t"))
            .and_then(J::as_i64)
            .unwrap_or(db.basis_tx);
        let report_val = json_to_mino(&report_json);
        let m = PMap::empty()
            .assoc(
                kw_ns("mentat.store", "db-after"),
                db_value(basis_after, None, None),
            )
            .assoc(kw_ns("mentat.store", "tx-report"), report_val);
        Ok(Value::Map(Gc::new(m)))
    });

    // q / q-once: (q db query). An as-of/since db forwards its bound into the
    // engine's temporal query inputs -- full Datalog on a historical basis.
    for name in ["mentat.store/q", "mentat.store/q-once"] {
        it.register_prim_fn(name, |_it, args| {
            let db = destructure_db("mentat.store/q", args.first())?;
            let query = args
                .get(1)
                .ok_or_else(|| throw_str("mentat.store/q: expected (db query)"))?;
            let edn = print_str(query);
            let inputs = temporal_inputs(&db);
            let out = super::query::mentat_query(&edn, pgrx::JsonB(inputs))
                .map_err(|e| throw_str(&format!("mentat.store/q: {e}")))?;
            Ok(query_result_value(&out.0))
        });
    }

    // pull: (pull db eid pattern) -> a map, or (pull db [eids] pattern) -> vec.
    it.register_prim_fn("mentat.store/pull", |_it, args| {
        let db = destructure_db("mentat.store/pull", args.first())?;
        let eid_arg = args
            .get(1)
            .ok_or_else(|| throw_str("mentat.store/pull: expected (db eid pattern)"))?;
        let pattern = args
            .get(2)
            .ok_or_else(|| throw_str("mentat.store/pull: expected (db eid pattern)"))?;
        let pat_edn = print_str(pattern);
        match eid_arg {
            Value::Vector(v) => {
                let eids: Result<Vec<i64>, Throw> = v
                    .iter()
                    .map(|e| resolve_eid("mentat.store/pull", &db, e))
                    .collect();
                let out = super::pull::mentat_pull_many(&pat_edn, eids?)
                    .map_err(|e| throw_str(&format!("mentat.store/pull: {e}")))?;
                Ok(json_to_mino(&out.0))
            }
            other => {
                let eid = resolve_eid("mentat.store/pull", &db, other)?;
                let out = super::pull::mentat_pull(&pat_edn, eid)
                    .map_err(|e| throw_str(&format!("mentat.store/pull: {e}")))?;
                Ok(json_to_mino(&out.0))
            }
        }
    });

    // entity: (entity db eid) -> {:db/id eid, :attr v, ...}.
    it.register_prim_fn("mentat.store/entity", |_it, args| {
        let db = destructure_db("mentat.store/entity", args.first())?;
        let eid_arg = args
            .get(1)
            .ok_or_else(|| throw_str("mentat.store/entity: expected (db eid)"))?;
        let eid = resolve_eid("mentat.store/entity", &db, eid_arg)?;
        let out = super::entity::mentat_entity(eid)
            .map_err(|e| throw_str(&format!("mentat.store/entity: {e}")))?;
        Ok(json_to_mino(&out.0))
    });

    // read: (read db eid attr) -> the value(s) for that eid/attr via q.
    it.register_prim_fn("mentat.store/read", |_it, args| {
        let db = destructure_db("mentat.store/read", args.first())?;
        let eid_arg = args
            .get(1)
            .ok_or_else(|| throw_str("mentat.store/read: expected (db eid attr)"))?;
        let attr_arg = args
            .get(2)
            .ok_or_else(|| throw_str("mentat.store/read: expected (db eid attr)"))?;
        let eid = resolve_eid("mentat.store/read", &db, eid_arg)?;
        let attr = match attr_arg {
            Value::Keyword(sym) => sym_to_kw_str(sym),
            _ => {
                return Err(throw_str(&format!(
                    "mentat.store/read: attr must be a keyword, got {}",
                    print_str(attr_arg)
                )))
            }
        };
        // A collection query -- the value(s) of that attr on that eid.
        let query = format!("[:find [?v ...] :where [{eid} {attr} ?v]]");
        let inputs = temporal_inputs(&db);
        let out = super::query::mentat_query(&query, pgrx::JsonB(inputs))
            .map_err(|e| throw_str(&format!("mentat.store/read: {e}")))?;
        // FindColl envelope: {"result": [..]}. Cardinality-one collapses to the
        // scalar; many stays a set; absent -> nil.
        let vals = out
            .0
            .get("result")
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default();
        match vals.len() {
            0 => Ok(Value::Nil),
            1 => Ok(json_to_mino(&vals[0])),
            _ => {
                let mut set = PSet::empty();
                for v in &vals {
                    set = set.conj(json_to_mino(v));
                }
                Ok(Value::Set(Gc::new(set)))
            }
        }
    });

    // datoms: (datoms db) -> a vector of [e a v tx added] tuples for the basis.
    it.register_prim_fn("mentat.store/datoms", |_it, args| {
        let db = destructure_db("mentat.store/datoms", args.first())?;
        datoms_value(&db)
    });

    // entities: (entities db attr) -> a set of eids that have that attribute.
    it.register_prim_fn("mentat.store/entities", |_it, args| {
        let db = destructure_db("mentat.store/entities", args.first())?;
        let attr_arg = args
            .get(1)
            .ok_or_else(|| throw_str("mentat.store/entities: expected (db attr)"))?;
        let attr = match attr_arg {
            Value::Keyword(sym) => sym_to_kw_str(sym),
            _ => {
                return Err(throw_str(&format!(
                    "mentat.store/entities: attr must be a keyword, got {}",
                    print_str(attr_arg)
                )))
            }
        };
        let query = format!("[:find [?e ...] :where [?e {attr} _]]");
        let inputs = temporal_inputs(&db);
        let out = super::query::mentat_query(&query, pgrx::JsonB(inputs))
            .map_err(|e| throw_str(&format!("mentat.store/entities: {e}")))?;
        let eids = out
            .0
            .get("result")
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default();
        let mut set = PSet::empty();
        for e in &eids {
            set = set.conj(json_to_mino(e));
        }
        Ok(Value::Set(Gc::new(set)))
    });

    // close: (close conn) -> nil. There is no owned connection to close (the
    // store is the current database); a no-op keeps API parity with Mentat so
    // ported scripts run unchanged.
    it.register_prim_fn("mentat.store/close", |_it, _args| Ok(Value::Nil));

    // as-of: (as-of db T) -> db' with :as-of T, basis clamped to T.
    it.register_prim_fn("mentat.store/as-of", |_it, args| {
        let db = destructure_db("mentat.store/as-of", args.first())?;
        let t = int_arg("mentat.store/as-of", args.get(1))?;
        Ok(db_value(t, Some(t), db.since))
    });

    // since: (since db T) -> db' with :since T.
    it.register_prim_fn("mentat.store/since", |_it, args| {
        let db = destructure_db("mentat.store/since", args.first())?;
        let t = int_arg("mentat.store/since", args.get(1))?;
        Ok(db_value(db.basis_tx, db.as_of, Some(t)))
    });
}

// ---------------------------------------------------------------------------
// db value construct / destructure
// ---------------------------------------------------------------------------

struct DbRef {
    basis_tx: i64,
    as_of: Option<i64>,
    since: Option<i64>,
}

fn db_value(basis_tx: i64, as_of: Option<i64>, since: Option<i64>) -> Value {
    let opt = |o: Option<i64>| o.map(Value::Int).unwrap_or(Value::Nil);
    let m = PMap::empty()
        .assoc(kw_ns("mentat.store", "db"), Value::Bool(true))
        .assoc(kw_ns("mentat.store", "conn"), Value::Int(1))
        .assoc(kw_ns("mentat.store", "basis-tx"), Value::Int(basis_tx))
        .assoc(kw_ns("mentat.store", "as-of"), opt(as_of))
        .assoc(kw_ns("mentat.store", "since"), opt(since));
    Value::Map(Gc::new(m))
}

/// Destructure a db value; a bare conn `Int` is accepted and treated as the
/// current db (basis re-read via SPI, as-of/since nil).
fn destructure_db(prim: &str, arg: Option<&Value>) -> Result<DbRef, Throw> {
    match arg {
        Some(Value::Int(_)) => {
            let basis = current_basis_tx().map_err(|e| throw_str(&e))?;
            Ok(DbRef {
                basis_tx: basis,
                as_of: None,
                since: None,
            })
        }
        Some(Value::Map(m)) => {
            let basis_tx = match m.get(&kw_ns("mentat.store", "basis-tx")) {
                Some(Value::Int(n)) => *n,
                _ => current_basis_tx().map_err(|e| throw_str(&e))?,
            };
            let opt = |k: &str| match m.get(&kw_ns("mentat.store", k)) {
                Some(Value::Int(n)) => Some(*n),
                _ => None,
            };
            Ok(DbRef {
                basis_tx,
                as_of: opt("as-of"),
                since: opt("since"),
            })
        }
        _ => Err(throw_str(&format!(
            "{prim}: first arg must be a db value (from mentat.store/db) or a conn handle"
        ))),
    }
}

/// The query-inputs JSON carrying a db value's temporal bound (empty for the
/// current basis). pg_mentat's `mentat_query` reads `asOf`/`since` from this.
fn temporal_inputs(db: &DbRef) -> J {
    let mut obj = serde_json::Map::new();
    if let Some(t) = db.as_of {
        obj.insert("asOf".to_string(), J::from(t));
    }
    if let Some(t) = db.since {
        obj.insert("since".to_string(), J::from(t));
    }
    J::Object(obj)
}

// ---------------------------------------------------------------------------
// Engine access over SPI
// ---------------------------------------------------------------------------

/// The current basis: the max committed transaction id, via SPI (read-only, so
/// it runs on a hot-standby too).
fn current_basis_tx() -> Result<i64, String> {
    Spi::get_one::<i64>("SELECT COALESCE(MAX(tx), 0) FROM mentat.transactions")
        .map(|o| o.unwrap_or(0))
        .map_err(|e| format!("current basis tx: {e}"))
}

/// Resolve an eid argument to a numeric entid. Accepts an integer, an ident
/// keyword (resolved via the schema cache), or a lookup-ref `[:attr val]`
/// (resolved with a scalar query, honoring the db's temporal bound).
fn resolve_eid(prim: &str, db: &DbRef, arg: &Value) -> Result<i64, Throw> {
    match arg {
        Value::Int(n) => Ok(*n),
        Value::Keyword(sym) => {
            let ident = sym_to_kw_str(sym);
            crate::cache::get_cache()
                .resolve_ident(&ident)
                .ok_or_else(|| throw_str(&format!("{prim}: unknown ident {ident}")))
        }
        Value::Vector(v) if v.len() == 2 => {
            // Lookup ref [:attr val] -> [:find ?e . :where [?e :attr val]].
            let attr = v.nth(0).unwrap();
            let val = v.nth(1).unwrap();
            let attr_kw = match attr {
                Value::Keyword(sym) => sym_to_kw_str(sym),
                _ => {
                    return Err(throw_str(&format!(
                        "{prim}: lookup-ref attr must be a keyword"
                    )))
                }
            };
            let query = format!("[:find ?e . :where [?e {attr_kw} {}]]", print_str(val));
            let inputs = temporal_inputs(db);
            let out = super::query::mentat_query(&query, pgrx::JsonB(inputs))
                .map_err(|e| throw_str(&format!("{prim}: lookup-ref query: {e}")))?;
            out.0.get("result").and_then(J::as_i64).ok_or_else(|| {
                throw_str(&format!(
                    "{prim}: lookup-ref {} resolved to no entity",
                    print_str(arg)
                ))
            })
        }
        _ => Err(throw_str(&format!(
            "{prim}: eid must be an integer, ident keyword, or [:attr val] lookup-ref, got {}",
            print_str(arg)
        ))),
    }
}

fn int_arg(prim: &str, arg: Option<&Value>) -> Result<i64, Throw> {
    match arg {
        Some(Value::Int(n)) => Ok(*n),
        _ => Err(throw_str(&format!("{prim}: expected an integer tx id"))),
    }
}

fn parse_json(prim: &str, s: &str) -> Result<J, Throw> {
    serde_json::from_str(s).map_err(|e| throw_str(&format!("{prim}: bad engine JSON: {e}")))
}

/// `(datoms db)` -> a vector of `[e a v tx added]` tuples for the basis, via a
/// history/temporal query over the current, as-of, or since bound.
fn datoms_value(db: &DbRef) -> Result<Value, Throw> {
    // Full EAVT scan; the engine applies the temporal bound from inputs.
    let query = "[:find ?e ?a ?v ?tx ?added :where [?e ?a ?v ?tx ?added]]";
    let mut inputs = temporal_inputs(db);
    // A datoms listing wants the raw datom log, so include retractions when
    // asking for a since/current window; as-of reconstructs the live set.
    if db.as_of.is_none() {
        if let J::Object(ref mut o) = inputs {
            o.insert("history".to_string(), J::from(true));
        }
    }
    let out = super::query::mentat_query(query, pgrx::JsonB(inputs))
        .map_err(|e| throw_str(&format!("mentat.store/datoms: {e}")))?;
    // FindRel envelope: {"results": [[e,a,v,tx,added], ...]}.
    let rows = out
        .0
        .get("results")
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    let tuples: Vec<Value> = rows.iter().map(json_to_mino).collect();
    Ok(Value::Vector(Gc::new(PVec::from_vec(tuples))))
}

// ---------------------------------------------------------------------------
// Value conversions
// ---------------------------------------------------------------------------

/// The tx report (engine JSON) as a mino map keyed under `:mentat.store/*`.
fn tx_report_value(report: &str) -> Result<Value, Throw> {
    let j = parse_json("mentat.store/transact", report)?;
    let tx_id = j
        .get("db-after")
        .and_then(|d| d.get("basis-t"))
        .and_then(J::as_i64)
        .unwrap_or(0);
    let mut tempids = PMap::empty();
    if let Some(obj) = j.get("tempids").and_then(J::as_object) {
        for (k, v) in obj {
            if let Some(n) = v.as_i64() {
                tempids = tempids.assoc(str_val(k), Value::Int(n));
            }
        }
    }
    let m = PMap::empty()
        .assoc(kw_ns("mentat.store", "tx-id"), Value::Int(tx_id))
        .assoc(
            kw_ns("mentat.store", "tempids"),
            Value::Map(Gc::new(tempids)),
        )
        .assoc(
            kw_ns("mentat.store", "db-after"),
            db_value(tx_id, None, None),
        );
    Ok(Value::Map(Gc::new(m)))
}

/// A `mentat_query` JSON envelope as a mino value in Datomic result shape:
/// scalar find -> the value; coll `[?x ...]` -> a vector; tuple `[?a ?b]` -> a
/// vector; relation `?a ?b` -> a set of tuple-vectors.
fn query_result_value(env: &J) -> Value {
    // Relation: has "results" (and "columns"); a set of tuple vectors.
    if let Some(rows) = env.get("results").and_then(J::as_array) {
        let mut set = PSet::empty();
        for row in rows {
            set = set.conj(json_to_mino(row));
        }
        return Value::Set(Gc::new(set));
    }
    // Scalar / coll / tuple: a single "result" value. A coll/tuple result is a
    // JSON array (becomes a vector); a scalar is a bare value.
    match env.get("result") {
        Some(v) => json_to_mino(v),
        None => Value::Nil,
    }
}

/// A `serde_json::Value` as a mino `Value`, RECURSIVELY. Restores keyword
/// typing (a string starting with `:` is a keyword) and maps object keys the
/// same way, so pull/entity maps keyed by `:person/name` become keyword-keyed
/// mino maps. Nested arrays/objects recurse (pull results are not collapsed).
fn json_to_mino(v: &J) -> Value {
    match v {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        J::String(s) => str_to_mino(s),
        J::Array(items) => Value::Vector(Gc::new(PVec::from_vec(
            items.iter().map(json_to_mino).collect(),
        ))),
        J::Object(entries) => {
            let mut m = PMap::empty();
            for (k, val) in entries {
                m = m.assoc(str_to_mino(k), json_to_mino(val));
            }
            Value::Map(Gc::new(m))
        }
    }
}

/// A JSON string as a mino value: a leading `:` makes it a keyword (pg_mentat
/// renders keyword/ref-ident values and attribute names as `:ns/name` text).
/// Otherwise a plain string.
fn str_to_mino(s: &str) -> Value {
    if let Some(rest) = s.strip_prefix(':') {
        if !rest.is_empty() {
            return Value::Keyword(kw_from_str(rest));
        }
    }
    str_val(s)
}

/// Build a mino keyword symbol from `ns/name` or `name`.
fn kw_from_str(s: &str) -> Symbol {
    match s.split_once('/') {
        Some((ns, name)) if !ns.is_empty() && !name.is_empty() => Symbol::namespaced(ns, name),
        _ => Symbol::plain(s),
    }
}

fn sym_to_kw_str(sym: &Symbol) -> String {
    match sym.ns.as_deref() {
        Some(ns) => format!(":{ns}/{}", sym.name),
        None => format!(":{}", sym.name),
    }
}

fn str_val(s: &str) -> Value {
    Value::Str(Gc::new(s.to_string()))
}

fn kw_ns(ns: &str, name: &str) -> Value {
    Value::Keyword(Symbol::namespaced(ns, name))
}

// ---------------------------------------------------------------------------
// Pure-value-conversion unit tests (no Postgres/SPI). These exercise the JSON
// -> mino bridge, which is the non-trivial logic in this module: keyword
// restoration, recursive nesting, and the Datomic result-shape mapping.
// Run under plain `cargo test -p pg_mentat --features script`.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod value_tests {
    use super::*;
    use mino_rs::printer::print_str;
    use serde_json::json;

    fn edn(v: &Value) -> String {
        print_str(v)
    }

    #[test]
    fn scalar_and_keyword_restore() {
        assert_eq!(edn(&json_to_mino(&json!(42))), "42");
        assert_eq!(edn(&json_to_mino(&json!(true))), "true");
        assert_eq!(edn(&json_to_mino(&json!("Alice"))), "\"Alice\"");
        // A leading ':' string is restored to a keyword, namespaced or plain.
        assert_eq!(edn(&json_to_mino(&json!(":person/name"))), ":person/name");
        assert_eq!(edn(&json_to_mino(&json!(":db.type/ref"))), ":db.type/ref");
        assert_eq!(edn(&json_to_mino(&json!(":name"))), ":name");
        // A bare ':' is not a keyword.
        assert_eq!(edn(&json_to_mino(&json!(":"))), "\":\"");
    }

    #[test]
    fn nested_map_is_not_collapsed() {
        // A pull-style nested result recurses; keys become keywords.
        let j = json!({":person/name": "Alice", ":person/friend": {":person/name": "Bob"}});
        let m = json_to_mino(&j);
        let s = edn(&m);
        assert!(s.contains(":person/name \"Alice\""), "{s}");
        assert!(s.contains(":person/friend {"), "nested map preserved: {s}");
        assert!(
            s.contains(":person/name \"Bob\""),
            "nested value preserved: {s}"
        );
    }

    #[test]
    fn query_shapes_map_to_datomic() {
        // Relation (has "results") -> a set of tuple vectors.
        let rel = json!({"columns": ["?n"], "results": [["Alice"], ["Bob"]], "result": [["Alice"], ["Bob"]]});
        let s = edn(&query_result_value(&rel));
        assert!(s.starts_with("#{"), "relation -> set: {s}");
        assert!(s.contains("[\"Alice\"]") && s.contains("[\"Bob\"]"), "{s}");

        // Scalar -> the bare value.
        let scalar = json!({"result": "Alice"});
        assert_eq!(edn(&query_result_value(&scalar)), "\"Alice\"");

        // Coll/tuple -> a vector.
        let coll = json!({"result": ["Alice", "Bob"]});
        assert_eq!(edn(&query_result_value(&coll)), "[\"Alice\" \"Bob\"]");

        // Empty result -> nil.
        assert_eq!(edn(&query_result_value(&json!({"result": null}))), "nil");
    }

    #[test]
    fn db_value_is_a_map_carrying_its_basis() {
        let s = edn(&db_value(0x1000_0002, None, None));
        assert!(s.contains(":mentat.store/db true"), "{s}");
        assert!(s.contains(":mentat.store/basis-tx 268435458"), "{s}");
        // as-of / since carry the temporal bound.
        let as_of = edn(&db_value(5, Some(5), None));
        assert!(as_of.contains(":mentat.store/as-of 5"), "{as_of}");
    }

    #[test]
    fn temporal_inputs_carry_the_bound() {
        let current = temporal_inputs(&DbRef {
            basis_tx: 9,
            as_of: None,
            since: None,
        });
        assert_eq!(current, json!({}));
        let as_of = temporal_inputs(&DbRef {
            basis_tx: 9,
            as_of: Some(7),
            since: None,
        });
        assert_eq!(as_of, json!({"asOf": 7}));
        let since = temporal_inputs(&DbRef {
            basis_tx: 9,
            as_of: None,
            since: Some(3),
        });
        assert_eq!(since, json!({"since": 3}));
    }

    #[test]
    fn tx_report_value_projects_the_report() {
        let report = r#"{"db-before":{"basis-t":1},"db-after":{"basis-t":268435458},"tx-data":[],"tempids":{"t":268435460},"schema_changed":false}"#;
        let s = edn(&tx_report_value(report).unwrap());
        assert!(s.contains(":mentat.store/tx-id 268435458"), "{s}");
        assert!(s.contains(":mentat.store/tempids"), "{s}");
        assert!(s.contains("\"t\" 268435460"), "tempid mapping: {s}");
        assert!(s.contains(":mentat.store/db-after"), "{s}");
    }

    #[test]
    fn float_and_null_and_nested_vec() {
        // A double survives as a float.
        assert_eq!(edn(&json_to_mino(&json!(3.5))), "3.5");
        // null -> nil.
        assert_eq!(edn(&json_to_mino(&json!(null))), "nil");
        // A nested vector of maps recurses without collapsing.
        let j = json!([{":a": 1}, {":a": 2}]);
        let s = edn(&json_to_mino(&j));
        assert!(s.starts_with('['), "{s}");
        assert!(s.contains(":a 1") && s.contains(":a 2"), "{s}");
    }

    #[test]
    fn keyword_arg_prints_back_as_ident_text() {
        // sym_to_kw_str is the inverse used to feed attr keywords to the engine.
        assert_eq!(
            sym_to_kw_str(&Symbol::namespaced("person", "name")),
            ":person/name"
        );
        assert_eq!(sym_to_kw_str(&Symbol::plain("name")), ":name");
    }

    #[test]
    fn keyword_round_trips_json_to_mino_and_back() {
        // A ref-ident value rendered by the engine as ":db.type/ref" text must
        // become a keyword whose printed form is identical (no double-colon,
        // no quoting).
        for ident in [":db.type/ref", ":person/name", ":done"] {
            let v = json_to_mino(&json!(ident));
            assert_eq!(edn(&v), ident, "round-trip for {ident}");
        }
    }

    #[test]
    fn empty_relation_is_the_empty_set() {
        let rel = json!({"columns": ["?n"], "results": [], "result": []});
        assert_eq!(edn(&query_result_value(&rel)), "#{}");
    }
}
