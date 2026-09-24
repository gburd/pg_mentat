// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use chrono::SecondsFormat;

use itertools::Itertools;
use pretty;

use std::borrow::Cow;
use std::io;

use crate::types::Value;

/// Render a string as an EDN text literal, escaping what `raw_text` cannot read
/// back literally.
///
/// The printer used to emit `"` + the raw string + `"`, which is not
/// round-trippable: an embedded `"` closed the literal early and an embedded `\`
/// began an escape the parser then rejected, so printing a value containing
/// either produced EDN that `parse::value` refused. Newlines and tabs are legal
/// raw inside a literal, so they are left alone rather than escaped.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Value {
    /// Return a pretty string representation of this `Value`.
    pub fn to_pretty(&self, width: usize) -> Result<String, io::Error> {
        let mut out = Vec::new();
        self.write_pretty(width, &mut out)?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Write a pretty representation of this `Value` to the given writer.
    fn write_pretty<W>(&self, width: usize, out: &mut W) -> Result<(), io::Error>
    where
        W: io::Write,
    {
        self.as_doc(&pretty::BoxAllocator).1.render(width, out)
    }

    /// Bracket a collection of values.
    ///
    /// We aim for
    /// [1 2 3]
    /// and fall back if necessary to
    /// [1,
    ///  2,
    ///  3].
    fn bracket<'a, A, T, I>(
        &'a self,
        allocator: &'a A,
        open: T,
        vs: I,
        close: T,
    ) -> pretty::DocBuilder<'a, A>
    where
        A: pretty::DocAllocator<'a>,
        <A as pretty::DocAllocator<'a>>::Doc: std::clone::Clone,
        T: Into<Cow<'a, str>>,
        I: IntoIterator<Item = &'a Value>,
    {
        let open = open.into();
        let n = open.len() as isize;
        let i = {
            let this = vs.into_iter().map(|v| v.as_doc(allocator));
            let element = allocator.line();
            Itertools::intersperse(this, element)
        };
        allocator
            .text(open)
            .append(allocator.concat(i).nest(n))
            .append(allocator.text(close))
            .group()
    }

    /// Recursively traverses this value and creates a pretty.rs document.
    /// A pretty printing implementation for edn queries optimized for
    /// readability and limited whitespace expansion.
    fn as_doc<'a, A>(&'a self, pp: &'a A) -> pretty::DocBuilder<'a, A>
    where
        A: pretty::DocAllocator<'a>,
        <A as pretty::DocAllocator<'a>>::Doc: std::clone::Clone,
    {
        match *self {
            Value::Vector(ref vs) => self.bracket(pp, "[", vs, "]"),
            Value::List(ref vs) => self.bracket(pp, "(", vs, ")"),
            Value::Set(ref vs) => self.bracket(pp, "#{", vs, "}"),
            Value::Map(ref vs) => {
                let xs = {
                    let this = vs
                        .iter()
                        .rev()
                        .map(|(k, v)| k.as_doc(pp).append(pp.line()).append(v.as_doc(pp)).group());
                    let element = pp.line();
                    Itertools::intersperse(this, element)
                };
                pp.text("{")
                    .append(pp.concat(xs).nest(1))
                    .append(pp.text("}"))
                    .group()
            }
            Value::NamespacedSymbol(ref v) => pp.text(v.namespace()).append("/").append(v.name()),
            Value::PlainSymbol(ref v) => pp.text(v.to_string()),
            Value::Keyword(ref v) => pp.text(v.to_string()),
            Value::Text(ref v) => pp.text(escape_text(v)),
            Value::Uuid(ref u) => pp
                .text("#uuid \"")
                .append(u.hyphenated().to_string())
                .append("\""),
            Value::Instant(ref v) => pp
                .text("#inst \"")
                .append(v.to_rfc3339_opts(SecondsFormat::AutoSi, true))
                .append("\""),
            _ => pp.text(self.to_string()),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::parse;

    #[test]
    fn test_pp_io() {
        let string = "$";
        let data = parse::value(string).unwrap().without_spans();

        assert_eq!(data.write_pretty(40, &mut Vec::new()).is_ok(), true);
    }

    #[test]
    fn test_pp_types_empty() {
        let string = "[ [ ] ( ) #{ } { }, \"\" ]";
        let data = parse::value(string).unwrap().without_spans();

        assert_eq!(data.to_pretty(40).unwrap(), "[[] () #{} {} \"\"]");
    }

    #[test]
    fn test_vector() {
        let string = "[1 2 3 4 5 6]";
        let data = parse::value(string).unwrap().without_spans();

        assert_eq!(data.to_pretty(20).unwrap(), "[1 2 3 4 5 6]");
        assert_eq!(
            data.to_pretty(10).unwrap(),
            "\
[1
 2
 3
 4
 5
 6]"
        );
    }

    #[test]
    fn test_map() {
        let string = "{:a 1 :b 2 :c 3}";
        let data = parse::value(string).unwrap().without_spans();

        assert_eq!(data.to_pretty(20).unwrap(), "{:a 1 :b 2 :c 3}");
        assert_eq!(
            data.to_pretty(10).unwrap(),
            "\
{:a 1
 :b 2
 :c 3}"
        );
    }

    #[test]
    fn test_pp_types() {
        let string = "[ 1 2 ( 3.14 ) #{ 4N } { foo/bar 42 :baz/boz 43 } [ ] :five :six/seven eight nine/ten true false nil #f NaN #f -Infinity #f +Infinity ]";
        let data = parse::value(string).unwrap().without_spans();

        assert_eq!(
            data.to_pretty(40).unwrap(),
            "\
[1
 2
 (3.14)
 #{4N}
 {:baz/boz 43 foo/bar 42}
 []
 :five
 :six/seven
 eight
 nine/ten
 true
 false
 nil
 #f NaN
 #f -Infinity
 #f +Infinity]"
        );
    }

    #[test]
    fn test_pp_query1() {
        let string = "[:find ?id ?bar ?baz :in $ :where [?id :session/keyword-foo ?symbol1 ?symbol2 \"some string\"] [?tx :db/tx ?ts]]";
        let data = parse::value(string).unwrap().without_spans();

        assert_eq!(
            data.to_pretty(40).unwrap(),
            "\
[:find
 ?id
 ?bar
 ?baz
 :in
 $
 :where
 [?id
  :session/keyword-foo
  ?symbol1
  ?symbol2
  \"some string\"]
 [?tx :db/tx ?ts]]"
        );
    }

    #[test]
    fn test_pp_query2() {
        let string = "[:find [?id ?bar ?baz] :in [$] :where [?id :session/keyword-foo ?symbol1 ?symbol2 \"some string\"] [?tx :db/tx ?ts] (not-join [?id] [?id :session/keyword-bar _])]";
        let data = parse::value(string).unwrap().without_spans();

        assert_eq!(
            data.to_pretty(40).unwrap(),
            "\
[:find
 [?id ?bar ?baz]
 :in
 [$]
 :where
 [?id
  :session/keyword-foo
  ?symbol1
  ?symbol2
  \"some string\"]
 [?tx :db/tx ?ts]
 (not-join
  [?id]
  [?id :session/keyword-bar _])]"
        );
    }
    /// `\n`/`\t`/`\r` must unescape to the control characters they denote.
    /// The rule previously echoed the character following the backslash, so
    /// these yielded the letters `n`/`t`/`r` and `"a\nb"` silently became
    /// `"anb"` -- data loss for any text carrying a newline or tab.
    #[test]
    fn test_string_escapes_unescape_to_control_characters() {
        let data = parse::value(r#""a\nb\tc\rd""#).unwrap().without_spans();
        assert_eq!(data, crate::Value::Text("a\nb\tc\rd".into()));
    }

    /// The passthrough arm: `\\` and `\"` are their own translation.
    #[test]
    fn test_backslash_and_quote_escapes_are_preserved() {
        let data = parse::value(r#""a\\b\"c""#).unwrap().without_spans();
        assert_eq!(data, crate::Value::Text("a\\b\"c".into()));
    }

    /// Printing then reparsing must be the identity. The printer emitted the
    /// raw string, so a value containing `"` closed its literal early and one
    /// containing `\` began an escape the parser rejected -- EDN this crate
    /// wrote, this crate could not read.
    #[test]
    fn test_text_round_trips_through_the_printer() {
        for original in [
            "plain",
            "has\"a quote",
            "has\\a backslash",
            "a\nnewline",
            "a\ttab",
            "all: \" \\ \n \t",
            "",
        ] {
            let value = crate::Value::Text(original.to_string());
            let printed = value.to_pretty(200).unwrap();
            let reparsed = parse::value(&printed)
                .unwrap_or_else(|e| panic!("printed {printed:?} is unparsable: {e}"))
                .without_spans();
            assert_eq!(reparsed, value, "round-trip changed {original:?}");
        }
    }
}
