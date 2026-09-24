// Copyright 2016-2018 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Nesting-depth limit for the parser.
//!
//! The `peg` grammar recurses once per nesting level, so the parser's stack use
//! grows with input nesting. Before this check, deeply nested input overflowed
//! the stack: in pg_mentat an unprivileged `mentat_query` with a vector nested
//! 3,000 deep killed the backend with SIGSEGV and forced a cluster-wide crash
//! recovery. Rust gives no tail-call guarantee and none of these recursions are
//! tail calls (each needs its children's results), so the fix is to refuse such
//! input up front with a linear scan that uses O(1) stack.
//!
//! Refusing it here also means no deeply nested `Value` is ever built, which
//! matters because the compiler-generated `Drop` of such a value recurses too.

use peg::error::{ExpectedSet, ParseError};
use peg::str::LineCol;

/// Deepest nesting of `(`, `[`, `{`, and `#{` any public parse entry point
/// accepts.
///
/// Measured stack cost per level (rustc 1.98, x86-64): about 2.9 KB
/// unoptimized, about 470 B in a release build. A 1 MB stack therefore holds
/// ~350 levels in a debug build and ~2,200 in release; PostgreSQL backends
/// commonly run with `max_stack_depth` = 2 MB. The deepest query or
/// transaction anywhere in mentat's and pg_mentat's test suites nests 5
/// levels. 256 leaves 50x headroom over real use and still fits a 1 MB stack
/// unoptimized (`tests/nesting.rs::limit_holds_on_a_small_stack`).
pub const MAX_NESTING: usize = 256;

/// Check that `input` never nests deeper than `max` levels.
///
/// Counts `(`, `[`, `{` (which also covers `#{`) and their closers, skipping
/// string literals (honouring `\"` and `\\` escapes) and `;` comments, the only
/// places the grammar lets those characters appear without meaning nesting.
/// Unbalanced closers are left for the grammar to report; this only bounds depth.
///
/// Linear time, constant stack.
pub fn check_nesting(input: &str, max: usize) -> Result<(), ParseError<LineCol>> {
    #[derive(PartialEq)]
    enum Lex {
        Code,
        Str,
        StrEscape,
        Comment,
    }
    let bytes = input.as_bytes();
    let mut state = Lex::Code;
    let mut depth: usize = 0;
    for (offset, &b) in bytes.iter().enumerate() {
        match state {
            Lex::Code => match b {
                b'(' | b'[' | b'{' => {
                    depth += 1;
                    if depth > max {
                        return Err(too_deep(input, offset, max));
                    }
                }
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b'"' => state = Lex::Str,
                b';' => state = Lex::Comment,
                _ => {}
            },
            Lex::Str => match b {
                b'\\' => state = Lex::StrEscape,
                b'"' => state = Lex::Code,
                _ => {}
            },
            Lex::StrEscape => state = Lex::Str,
            Lex::Comment => {
                if b == b'\n' || b == b'\r' {
                    state = Lex::Code;
                }
            }
        }
    }
    Ok(())
}

fn too_deep(input: &str, offset: usize, max: usize) -> ParseError<LineCol> {
    let before = &input[..offset];
    let line = before.bytes().filter(|&b| b == b'\n').count() + 1;
    let column = offset - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    ParseError {
        location: LineCol {
            line,
            column,
            offset,
        },
        expected: expected_set(max),
    }
}

/// `ExpectedSet` has no public constructor; `peg` builds one only through a
/// failed parse. Produce the message through a one-rule grammar whose sole
/// alternative is `expected!(<message>)`. The message needs a `'static` str, so
/// the limit is baked in: callers use `MAX_NESTING` or one of these fixed values.
fn expected_set(max: usize) -> ExpectedSet {
    peg::parser!(grammar limit() for str {
        pub rule default() = expected!("nesting depth at most 256")
        pub rule pg_value() = expected!("nesting depth at most 100")
        pub rule other() = expected!("less nesting (input nests too deeply)")
    });
    let r = match max {
        MAX_NESTING => limit::default(""),
        100 => limit::pg_value(""),
        _ => limit::other(""),
    };
    r.expect_err("expected! always fails").expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_every_opener() {
        assert!(check_nesting("[({#{}})]", 4).is_ok());
        assert!(check_nesting("[({#{}})]", 3).is_err());
    }

    #[test]
    fn strings_and_comments_are_opaque() {
        assert!(check_nesting(r#"["[[[[" "\"[[[[" "\\" ]"#, 1).is_ok());
        assert!(check_nesting("[ ; [[[[\n ]", 1).is_ok());
        assert!(check_nesting("[ ; [[[[\r ]", 1).is_ok());
    }

    #[test]
    fn escaped_backslash_ends_the_string() {
        // "\\" is a complete string; the [ after it is code.
        assert!(check_nesting(r#"["\\" [ ]]"#, 1).is_err());
        assert!(check_nesting(r#"["\\" [ ]]"#, 2).is_ok());
    }

    #[test]
    fn error_names_the_limit_and_position() {
        let e = check_nesting("[\n [[", MAX_NESTING.min(2)).unwrap_err();
        assert_eq!((e.location.line, e.location.column), (2, 3));
        let e = check_nesting(&"[".repeat(MAX_NESTING + 1), MAX_NESTING).unwrap_err();
        assert!(e.to_string().contains("nesting depth at most 256"), "{e}");
    }

    #[test]
    fn unbalanced_closers_do_not_underflow() {
        assert!(check_nesting("]]]][", 1).is_ok());
    }
}
