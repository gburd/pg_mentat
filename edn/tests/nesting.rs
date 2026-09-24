// Nesting-depth regression tests.
//
// The grammar recurses once per nesting level, so unbounded input nesting
// overflowed the stack: in pg_mentat an unprivileged `mentat_query` with a
// vector nested 3,000 deep killed the backend with SIGSEGV and forced a
// cluster-wide crash recovery. Every public parse entry point now rejects input
// nested deeper than `edn::MAX_NESTING` before the grammar sees it.
//
// This is its own test binary (own process): before the fix these tests abort
// the process instead of failing, which would take other test files down too.

use edn::parse;

fn nested(open: &str, close: &str, n: usize) -> String {
    format!("{}{}", open.repeat(n), close.repeat(n))
}

fn assert_nesting_error<T: std::fmt::Debug>(r: Result<T, edn::ParseError>, what: &str) {
    let e = r.expect_err(what);
    let msg = e.to_string();
    assert!(msg.contains("nesting"), "{what}: unexpected error: {msg}");
}

#[test]
fn deep_value_is_an_error_not_a_crash() {
    for (open, close) in [("[", "]"), ("(", ")"), ("{", "}"), ("#{", "}")] {
        assert_nesting_error(parse::value(&nested(open, close, 100_000)), open);
    }
}

#[test]
fn deep_query_is_an_error() {
    let q = format!("[:find ?e :where [?e :a/b {}]]", nested("[", "]", 100_000));
    assert_nesting_error(parse::parse_query(&q), "query");
}

#[test]
fn deep_transaction_is_an_error() {
    let tx = format!("[[:db/add 1 :a/b {}]]", nested("[", "]", 100_000));
    assert_nesting_error(parse::entities(&tx), "entities");
    assert_nesting_error(
        parse::entity(&format!("[:db/add 1 :a/b {}]", nested("[", "]", 100_000))),
        "entity",
    );
}

#[test]
fn limit_is_exact() {
    let n = edn::MAX_NESTING;
    assert!(
        parse::value(&nested("[", "]", n)).is_ok(),
        "{n} levels must parse"
    );
    assert_nesting_error(parse::value(&nested("[", "]", n + 1)), "one over the limit");
}

#[test]
fn mixed_delimiters_count_together() {
    let over = edn::MAX_NESTING / 4 + 1;
    let s = format!("{}{}", "[({#{".repeat(over), "}})]".repeat(over));
    assert_nesting_error(parse::value(&s), "mixed");
}

#[test]
fn delimiters_in_strings_and_comments_dont_count() {
    let deep = "[".repeat(5 * edn::MAX_NESTING);
    let s = format!("[\"{deep}\" ; {deep}\n \"\\\"{deep}\" ]");
    let v = parse::value(&s).expect("brackets inside strings/comments are not nesting");
    assert_eq!(v.without_spans().as_vector().map(|v| v.len()), Some(2));
}

#[test]
fn wide_is_fine() {
    let s = format!("[{}]", "[] ".repeat(1_000_000));
    assert!(parse::value(&s).is_ok());
}

#[test]
fn limit_holds_on_a_small_stack() {
    // PostgreSQL backends often run with max_stack_depth = 2 MB. The largest
    // accepted input must parse on a 1 MB stack even in an unoptimized build.
    let r = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| parse::value(&nested("[", "]", edn::MAX_NESTING)).is_ok())
        .unwrap()
        .join();
    assert_eq!(
        r.ok(),
        Some(true),
        "MAX_NESTING levels overflowed a 1 MB stack"
    );
}
