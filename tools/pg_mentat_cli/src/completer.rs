use std::sync::{Arc, RwLock};

use linefeed::complete::{Completer, Completion};
use linefeed::prompter::Prompter;
use linefeed::terminal::Terminal;

/// Dot-commands available in the REPL.
static DOT_COMMANDS: &[&str] = &[
    ".cache_stats",
    ".clear_cache",
    ".entity",
    ".exit",
    ".explain",
    ".export",
    ".help",
    ".import",
    ".quit",
    ".schema",
    ".sql",
    ".stats",
    ".storage",
    ".timer",
];

/// Common Datalog keywords used in queries and patterns.
static DATALOG_KEYWORDS: &[&str] = &[
    ":db/id",
    ":db/ident",
    ":db/valueType",
    ":db/cardinality",
    ":db/unique",
    ":db/index",
    ":db/fulltext",
    ":db/isComponent",
    ":db/noHistory",
    ":db/add",
    ":db/retract",
    ":db.type/string",
    ":db.type/long",
    ":db.type/boolean",
    ":db.type/ref",
    ":db.type/double",
    ":db.type/instant",
    ":db.type/uuid",
    ":db.type/keyword",
    ":db.type/bytes",
    ":db.cardinality/one",
    ":db.cardinality/many",
    ":db.unique/value",
    ":db.unique/identity",
    ":find",
    ":where",
    ":in",
    ":with",
    ":limit",
    ":offset",
];

/// Tab completer for the pg_mentat REPL.
///
/// Completes dot-commands, common Datalog keywords, and dynamically loaded
/// schema attribute names.
pub struct MentatCompleter {
    /// Dynamically loaded attribute idents from the database schema.
    schema_idents: Arc<RwLock<Vec<String>>>,
}

impl MentatCompleter {
    pub fn new() -> Self {
        Self {
            schema_idents: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Update the cached schema idents. Call this after connecting or after schema changes.
    pub fn set_schema_idents(&self, idents: Vec<String>) {
        if let Ok(mut cache) = self.schema_idents.write() {
            *cache = idents;
        }
    }
}

impl<Term: Terminal> Completer<Term> for MentatCompleter {
    fn complete(
        &self,
        word: &str,
        _prompter: &Prompter<Term>,
        _start: usize,
        _end: usize,
    ) -> Option<Vec<Completion>> {
        let mut completions = Vec::new();

        if word.starts_with('.') {
            // Complete dot-commands
            for &cmd in DOT_COMMANDS {
                if cmd.starts_with(word) {
                    completions.push(Completion::simple(cmd.to_string()));
                }
            }
        } else if word.starts_with(':') {
            // Complete keywords and schema idents
            for &kw in DATALOG_KEYWORDS {
                if kw.starts_with(word) {
                    completions.push(Completion::simple(kw.to_string()));
                }
            }

            // Also check dynamically loaded schema idents
            if let Ok(idents) = self.schema_idents.read() {
                for ident in idents.iter() {
                    if ident.starts_with(word) {
                        completions.push(Completion::simple(ident.clone()));
                    }
                }
            }
        }

        if completions.is_empty() {
            None
        } else {
            Some(completions)
        }
    }
}
