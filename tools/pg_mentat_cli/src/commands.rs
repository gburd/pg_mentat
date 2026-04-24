/// Commands supported by the pg_mentat CLI REPL.

#[derive(Clone, Debug)]
pub enum Command {
    /// Show help text
    Help,
    /// Show all schema attributes
    Schema,
    /// Show database statistics
    Stats,
    /// Show storage statistics
    StorageStats,
    /// Exit the REPL
    Exit,
    /// Execute a Datalog query: [:find ...]
    Query(String),
    /// Execute a transaction: [{:db/ident ...}]
    Transact(String),
    /// Execute a pull: (pull <entity_id> <pattern>)
    Pull(String, i64),
    /// Execute a pull-many: (pull-many <pattern> [id1 id2 ...])
    PullMany(String, Vec<i64>),
    /// Execute raw SQL
    Sql(String),
    /// Toggle timing on/off
    Timer(bool),
    /// Show entity by id
    Entity(i64),
    /// Clear prepared statement cache
    ClearCache,
    /// Show prepared statement cache stats
    CacheStats,
    /// Show explain plan for a Datalog query
    Explain(String),
    /// Export entities to EDN
    Export(Vec<i64>),
    /// Import EDN transaction data
    Import(String),
}

/// Parse a line of input into a Command.
/// Returns None if the line is empty or needs more input.
/// Returns Some(Err(...)) for parse errors.
pub fn parse_line(line: &str) -> Option<Result<Command, String>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Dot-commands
    if trimmed.starts_with('.') {
        let rest = &trimmed[1..];
        let mut parts = rest.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("");
        let args = parts.next().unwrap_or("").trim();

        return Some(match cmd {
            "help" | "h" => Ok(Command::Help),
            "schema" => Ok(Command::Schema),
            "stats" => Ok(Command::Stats),
            "storage" => Ok(Command::StorageStats),
            "exit" | "quit" | "q" => Ok(Command::Exit),
            "timer" => match args {
                "on" => Ok(Command::Timer(true)),
                "off" => Ok(Command::Timer(false)),
                _ => Err("Usage: .timer on|off".to_string()),
            },
            "entity" => match args.parse::<i64>() {
                Ok(id) => Ok(Command::Entity(id)),
                Err(_) => Err("Usage: .entity <entity_id>".to_string()),
            },
            "clear_cache" => Ok(Command::ClearCache),
            "cache_stats" => Ok(Command::CacheStats),
            "sql" if !args.is_empty() => Ok(Command::Sql(args.to_string())),
            "sql" => Err("Usage: .sql <SQL statement>".to_string()),
            "explain" | "eq" if !args.is_empty() => Ok(Command::Explain(args.to_string())),
            "explain" | "eq" => Err("Usage: .explain [:find ?e :where ...]".to_string()),
            "export" if !args.is_empty() => {
                let ids: Result<Vec<i64>, _> = args
                    .split_whitespace()
                    .map(|s| s.trim_matches(',').parse::<i64>())
                    .collect();
                match ids {
                    Ok(v) => Ok(Command::Export(v)),
                    Err(_) => Err("Usage: .export <id1> <id2> ...".to_string()),
                }
            }
            "export" => Err("Usage: .export <entity_id1> <entity_id2> ...".to_string()),
            "import" if !args.is_empty() => Ok(Command::Import(args.to_string())),
            "import" => Err("Usage: .import <filepath>".to_string()),
            other => Err(format!("Unknown command: .{other}")),
        });
    }

    // EDN-style input: detect query vs transaction
    if trimmed.starts_with("[:find") || trimmed.starts_with("[:find") {
        return Some(Ok(Command::Query(trimmed.to_string())));
    }

    if trimmed.starts_with("[{") || trimmed.starts_with("{") {
        return Some(Ok(Command::Transact(trimmed.to_string())));
    }

    // Pull-many syntax: (pull-many <pattern> [id1 id2 ...])
    if trimmed.starts_with("(pull-many ") {
        return parse_pull_many(trimmed);
    }

    // Pull syntax: (pull <entity_id> <pattern>)
    if trimmed.starts_with("(pull ") {
        return parse_pull(trimmed);
    }

    // Bare bracket expressions that look like Datalog
    if trimmed.starts_with("[:") {
        return Some(Ok(Command::Query(trimmed.to_string())));
    }

    // Anything else starting with SELECT/INSERT/etc. treat as SQL
    let upper = trimmed.to_uppercase();
    if upper.starts_with("SELECT")
        || upper.starts_with("INSERT")
        || upper.starts_with("UPDATE")
        || upper.starts_with("DELETE")
        || upper.starts_with("CREATE")
        || upper.starts_with("DROP")
        || upper.starts_with("ALTER")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("WITH")
    {
        return Some(Ok(Command::Sql(trimmed.to_string())));
    }

    Some(Err(format!(
        "Unrecognized input. Use .help for available commands.\n\
         Datalog queries start with [:find ...], transactions with [{{ ... }}]"
    )))
}

fn parse_pull(input: &str) -> Option<Result<Command, String>> {
    // Expected format: (pull <entity_id> <pattern>)
    // e.g., (pull 42 [*])  or  (pull 42 [:db/ident :db/valueType])
    let inner = input
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let without_pull = inner.strip_prefix("pull").unwrap_or(inner).trim();

    let mut chars = without_pull.chars().peekable();
    // Read entity id
    let mut id_str = String::new();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() || ch == '-' {
            id_str.push(ch);
            chars.next();
        } else {
            break;
        }
    }

    let entity_id = match id_str.parse::<i64>() {
        Ok(id) => id,
        Err(_) => return Some(Err("Pull syntax: (pull <entity_id> <pattern>)".to_string())),
    };

    let pattern: String = chars.collect();
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Some(Err("Pull syntax: (pull <entity_id> <pattern>)".to_string()));
    }

    Some(Ok(Command::Pull(pattern.to_string(), entity_id)))
}

fn parse_pull_many(input: &str) -> Option<Result<Command, String>> {
    // Expected format: (pull-many <pattern> [id1 id2 ...])
    // e.g., (pull-many [:person/name :person/age] [100 101 102])
    let inner = input
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let without_cmd = inner.strip_prefix("pull-many").unwrap_or(inner).trim();

    // Find the pattern (first balanced [...]) and then the entity ID list (second [...])
    let (pattern, rest) = match extract_bracket_expr(without_cmd) {
        Some(pair) => pair,
        None => return Some(Err("Pull-many syntax: (pull-many [:attr ...] [id1 id2 ...])".to_string())),
    };

    let rest = rest.trim();
    let (ids_str, _) = match extract_bracket_expr(rest) {
        Some(pair) => pair,
        None => return Some(Err("Pull-many syntax: (pull-many [:attr ...] [id1 id2 ...])".to_string())),
    };

    // Parse entity IDs from the bracket expression (strip brackets)
    let ids_inner = &ids_str[1..ids_str.len() - 1];
    let ids: Result<Vec<i64>, _> = ids_inner
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_matches(',').parse::<i64>())
        .collect();

    match ids {
        Ok(v) if v.is_empty() => Some(Err("Pull-many requires at least one entity ID".to_string())),
        Ok(v) => Some(Ok(Command::PullMany(pattern, v))),
        Err(_) => Some(Err("Entity IDs must be integers. Syntax: (pull-many [:attr ...] [100 101])".to_string())),
    }
}

/// Extract the first balanced bracket expression from the input.
/// Returns (bracket_expr_including_brackets, remaining_input).
fn extract_bracket_expr(input: &str) -> Option<(String, &str)> {
    let input = input.trim();
    if !input.starts_with('[') {
        return None;
    }

    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in input.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_string => escape = true,
            '"' => in_string = !in_string,
            '[' if !in_string => depth += 1,
            ']' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some((input[..=i].to_string(), &input[i + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

/// Returns true if the given input appears to be an incomplete multi-line entry
/// (unbalanced brackets/braces).
pub fn is_incomplete(input: &str) -> bool {
    let mut brackets = 0i32;
    let mut braces = 0i32;
    let mut parens = 0i32;
    let mut in_string = false;
    let mut escape = false;

    for ch in input.chars() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_string => escape = true,
            '"' => in_string = !in_string,
            '[' if !in_string => brackets += 1,
            ']' if !in_string => brackets -= 1,
            '{' if !in_string => braces += 1,
            '}' if !in_string => braces -= 1,
            '(' if !in_string => parens += 1,
            ')' if !in_string => parens -= 1,
            _ => {}
        }
    }

    brackets > 0 || braces > 0 || parens > 0
}
