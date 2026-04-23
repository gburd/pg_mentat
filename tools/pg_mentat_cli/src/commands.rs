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
    /// Execute a pull: (pull ?e [:attr ...])
    Pull(String, i64),
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
