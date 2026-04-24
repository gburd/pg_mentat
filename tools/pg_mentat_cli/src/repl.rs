use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use linefeed::{Interface, ReadResult, Signal};
use postgres::{Client, NoTls};
use tabwriter::TabWriter;
use termion::{color, style};

use crate::commands::{self, Command};
use crate::completer::MentatCompleter;

static BLUE: color::Rgb = color::Rgb(0x99, 0xaa, 0xFF);
static GREEN: color::Rgb = color::Rgb(0x77, 0xFF, 0x99);
static RED: color::Rgb = color::Rgb(0xFF, 0x66, 0x66);

const DEFAULT_PROMPT: &str = "mentat=> ";
const MORE_PROMPT: &str = "mentat-> ";
const HISTORY_FILE: &str = ".pg_mentat_history";

fn history_file_path() -> PathBuf {
    let mut p = dirs::home_dir().unwrap_or_default();
    p.push(HISTORY_FILE);
    p
}

/// The REPL state.
pub struct Repl {
    client: Client,
    interface: Option<Interface<linefeed::DefaultTerminal>>,
    completer: Arc<MentatCompleter>,
    buffer: String,
    timer_on: bool,
    conninfo: String,
}

impl Repl {
    /// Create a new REPL connected to PostgreSQL.
    pub fn new(conninfo: &str, use_tty: bool) -> Result<Repl, String> {
        let mut client = Client::connect(conninfo, NoTls)
            .map_err(|e| format!("Failed to connect to PostgreSQL: {e}"))?;

        let completer = Arc::new(MentatCompleter::new());

        let interface = if use_tty {
            let iface = Interface::new("pg_mentat")
                .map_err(|_| "Failed to create TTY interface; try --no-tty")?;
            {
                let mut r = iface.lock_reader();
                r.set_report_signal(Signal::Interrupt, true);
                // Colons are NOT word-break chars so :person/name completes as one word
                r.set_word_break_chars(" \t\n!\"#$%&'(){}*+,-;<=>?@[\\]^`");
            }
            iface.set_completer(Arc::clone(&completer));
            // Load history
            let p = history_file_path();
            let _ = iface.load_history(&p);
            Some(iface)
        } else {
            None
        };

        // Load schema idents for tab completion
        load_schema_idents(&mut client, &completer);

        Ok(Repl {
            client,
            interface,
            completer,
            buffer: String::new(),
            timer_on: false,
            conninfo: conninfo.to_string(),
        })
    }

    /// Run a single command without entering the REPL loop.
    pub fn run_command(&mut self, cmd: Command) {
        self.handle_command(cmd);
    }

    /// Run the interactive REPL loop.
    pub fn run(&mut self) {
        eprintln!(
            "{}pg_mentat CLI{} - interactive Datalog shell for PostgreSQL",
            style::Bold,
            style::Reset
        );
        eprintln!("Connected to: {}", self.conninfo);
        eprintln!("Type .help for available commands.\n");

        loop {
            let prompt = if self.buffer.is_empty() {
                DEFAULT_PROMPT
            } else {
                MORE_PROMPT
            };

            let colored_prompt = format!(
                "{blue}{prompt}{reset}",
                blue = color::Fg(BLUE),
                prompt = prompt,
                reset = color::Fg(color::Reset)
            );

            match self.read_line(&colored_prompt) {
                LineResult::Input(line) => {
                    if !self.buffer.is_empty() {
                        self.buffer.push('\n');
                    }
                    self.buffer.push_str(&line);

                    if self.buffer.trim().is_empty() {
                        self.buffer.clear();
                        continue;
                    }

                    // Check for multi-line input
                    if commands::is_incomplete(&self.buffer) {
                        continue;
                    }

                    let input = self.buffer.clone();
                    self.buffer.clear();
                    self.add_history(&input);

                    match commands::parse_line(&input) {
                        Some(Ok(cmd)) => {
                            if !self.handle_command(cmd) {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            self.print_error(&e);
                        }
                        None => {}
                    }
                }
                LineResult::Interrupt => {
                    if !self.buffer.is_empty() {
                        self.buffer.clear();
                        eprintln!();
                    } else {
                        break;
                    }
                }
                LineResult::Eof => {
                    if self.is_tty() {
                        eprintln!();
                    }
                    break;
                }
            }
        }

        self.save_history();
    }

    /// Handle a single command. Returns false if the REPL should exit.
    fn handle_command(&mut self, cmd: Command) -> bool {
        let start = if self.timer_on {
            Some(Instant::now())
        } else {
            None
        };

        match cmd {
            Command::Help => self.print_help(),
            Command::Schema => self.show_schema(),
            Command::Stats => self.show_stats(),
            Command::StorageStats => self.show_storage_stats(),
            Command::Exit => {
                eprintln!("Goodbye.");
                return false;
            }
            Command::Query(q) => self.execute_query(&q),
            Command::Transact(t) => self.execute_transact(&t),
            Command::Pull(pattern, entity_id) => self.execute_pull(&pattern, entity_id),
            Command::PullMany(pattern, entity_ids) => self.execute_pull_many(&pattern, &entity_ids),
            Command::Sql(sql) => self.execute_sql(&sql),
            Command::Timer(on) => {
                self.timer_on = on;
                eprintln!("Timer {}.", if on { "on" } else { "off" });
            }
            Command::Entity(id) => self.show_entity(id),
            Command::ClearCache => self.clear_cache(),
            Command::CacheStats => self.show_cache_stats(),
            Command::Explain(query) => self.explain_query(&query),
            Command::Export(entity_ids) => self.export_entities(&entity_ids),
            Command::Import(path) => self.import_file(&path),
        }

        if let Some(start) = start {
            self.print_timing(start.elapsed());
        }

        true
    }

    // -- Command implementations --

    fn print_help(&self) {
        let stdout = io::stdout();
        let mut tw = TabWriter::new(stdout.lock());
        let _ = writeln!(tw);
        let _ = writeln!(tw, "{}Commands:{}", style::Bold, style::Reset);
        let _ = writeln!(tw, "  .help, .h\tShow this help message");
        let _ = writeln!(tw, "  .schema\tShow all schema attributes");
        let _ = writeln!(tw, "  .stats\tShow query and function statistics");
        let _ = writeln!(tw, "  .storage\tShow storage statistics");
        let _ = writeln!(tw, "  .entity <id>\tShow all datoms for an entity");
        let _ = writeln!(tw, "  .timer on|off\tToggle query timing");
        let _ = writeln!(tw, "  .cache_stats\tShow prepared statement cache stats");
        let _ = writeln!(tw, "  .clear_cache\tClear prepared statement cache");
        let _ = writeln!(tw, "  .explain <query>\tShow EXPLAIN ANALYZE for a Datalog query");
        let _ = writeln!(tw, "  .export <id> ...\tExport entities as JSON (pull [*])");
        let _ = writeln!(tw, "  .import <file>\tImport an EDN transaction file");
        let _ = writeln!(tw, "  .sql <stmt>\tExecute raw SQL");
        let _ = writeln!(tw, "  .exit, .quit\tExit the REPL");
        let _ = writeln!(tw);
        let _ = writeln!(tw, "{}Datalog:{}", style::Bold, style::Reset);
        let _ = writeln!(
            tw,
            "  [:find ?e :where [?e :db/ident _]]\tExecute a Datalog query"
        );
        let _ = writeln!(
            tw,
            "  [{{:db/ident :foo/bar ...}}]\tExecute a transaction"
        );
        let _ = writeln!(tw, "  (pull 42 [*])\tPull entity attributes");
        let _ = writeln!(tw, "  (pull-many [:attr] [1 2 3])\tBatched pull of multiple entities");
        let _ = writeln!(tw);
        let _ = tw.flush();
    }

    fn show_schema(&mut self) {
        match self.client.query_one("SELECT mentat_schema()", &[]) {
            Ok(row) => {
                let json: serde_json::Value =
                    serde_json::from_str(&format!("{}", row.get::<_, String>(0)))
                        .unwrap_or_else(|_| {
                            // Try getting as postgres JsonB
                            serde_json::Value::String(row.get::<_, String>(0))
                        });
                self.pretty_print_json(&json);
            }
            Err(e) => self.print_error(&format!("Schema query failed: {e}")),
        }
    }

    fn show_stats(&mut self) {
        match self.client.query_one("SELECT mentat_query_stats()", &[]) {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.pretty_print_json(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Stats query failed: {e}")),
        }
    }

    fn show_storage_stats(&mut self) {
        match self
            .client
            .query_one("SELECT mentat_storage_stats()", &[])
        {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.pretty_print_json(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Storage stats query failed: {e}")),
        }
    }

    fn execute_query(&mut self, query: &str) {
        match self
            .client
            .query_one("SELECT mentat_query($1, $2)", &[&query, &"{}"])
        {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.print_query_results(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Query failed: {e}")),
        }
    }

    fn execute_transact(&mut self, transaction: &str) {
        match self
            .client
            .query_one("SELECT mentat_transact($1)", &[&transaction])
        {
            Ok(row) => {
                let raw: String = row.get(0);
                self.print_success(&format!("Transaction result: {raw}"));
                // Refresh tab completions in case new schema attributes were defined
                load_schema_idents(&mut self.client, &self.completer);
            }
            Err(e) => self.print_error(&format!("Transaction failed: {e}")),
        }
    }

    fn execute_pull(&mut self, pattern: &str, entity_id: i64) {
        match self
            .client
            .query_one("SELECT mentat_pull($1, $2)", &[&pattern, &entity_id])
        {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.pretty_print_json(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Pull failed: {e}")),
        }
    }

    fn execute_pull_many(&mut self, pattern: &str, entity_ids: &[i64]) {
        // Build the ARRAY literal for the entity IDs
        let ids_sql: String = entity_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT mentat_pull_many($1, ARRAY[{}]::BIGINT[])",
            ids_sql
        );

        match self.client.query_one(&sql, &[&pattern]) {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.pretty_print_json(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Pull-many failed: {e}")),
        }
    }

    fn explain_query(&mut self, query: &str) {
        // Use EXPLAIN ANALYZE on the generated SQL from mentat_query
        // First try to get the SQL plan
        let explain_sql = format!("EXPLAIN (ANALYZE, FORMAT JSON) SELECT mentat_query($1, $2)");
        match self.client.query_one(&explain_sql, &[&query, &"{}"]) {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.pretty_print_json(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Explain failed: {e}")),
        }
    }

    fn export_entities(&mut self, entity_ids: &[i64]) {
        // Pull each entity with [*] pattern and output as EDN-style data
        let ids_sql: String = entity_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT mentat_pull_many('[*]', ARRAY[{}]::BIGINT[])",
            ids_sql
        );

        match self.client.query_one(&sql, &[]) {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => {
                        // Output as pretty JSON (can be re-imported as transaction data)
                        self.pretty_print_json(&json);
                    }
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Export failed: {e}")),
        }
    }

    fn import_file(&mut self, path: &str) {
        // Read the file and treat its contents as a transaction
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let trimmed = contents.trim();
                if trimmed.is_empty() {
                    self.print_error("File is empty");
                    return;
                }
                self.print_success(&format!("Importing from: {path}"));
                self.execute_transact(trimmed);
            }
            Err(e) => self.print_error(&format!("Failed to read file: {e}")),
        }
    }

    fn execute_sql(&mut self, sql: &str) {
        match self.client.query(sql, &[]) {
            Ok(rows) => {
                if rows.is_empty() {
                    self.print_success("OK (0 rows)");
                    return;
                }

                let stdout = io::stdout();
                let mut tw = TabWriter::new(stdout.lock());

                // Print column headers
                let columns = rows[0].columns();
                for col in columns {
                    let _ = write!(tw, "| {}\t", col.name());
                }
                let _ = writeln!(tw, "|");

                // Separator
                for _ in columns {
                    let _ = write!(tw, "---\t");
                }
                let _ = writeln!(tw);

                // Print rows
                for row in &rows {
                    for (i, col) in columns.iter().enumerate() {
                        let val = get_column_display(&row, i, col);
                        let _ = write!(tw, "| {}\t", val);
                    }
                    let _ = writeln!(tw, "|");
                }

                // Footer
                for _ in columns {
                    let _ = write!(tw, "---\t");
                }
                let _ = writeln!(tw);
                let _ = writeln!(tw, "({} row{})", rows.len(), if rows.len() == 1 { "" } else { "s" });
                let _ = tw.flush();
            }
            Err(e) => self.print_error(&format!("SQL error: {e}")),
        }
    }

    fn show_entity(&mut self, entity_id: i64) {
        match self
            .client
            .query_one("SELECT mentat_entity($1)", &[&entity_id])
        {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.pretty_print_json(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Entity lookup failed: {e}")),
        }
    }

    fn clear_cache(&mut self) {
        match self
            .client
            .query_one("SELECT mentat_stmt_cache_clear()", &[])
        {
            Ok(_) => self.print_success("Prepared statement cache cleared."),
            Err(e) => self.print_error(&format!("Cache clear failed: {e}")),
        }
    }

    fn show_cache_stats(&mut self) {
        match self
            .client
            .query_one("SELECT mentat_stmt_cache_stats()", &[])
        {
            Ok(row) => {
                let raw: String = row.get(0);
                match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(json) => self.pretty_print_json(&json),
                    Err(_) => println!("{raw}"),
                }
            }
            Err(e) => self.print_error(&format!("Cache stats failed: {e}")),
        }
    }

    // -- Display helpers --

    fn print_query_results(&self, json: &serde_json::Value) {
        // The mentat_query function returns JSON with results.
        // Try to display as a table if it's an array of arrays or objects.
        if let Some(results) = json.get("results").or(Some(json)) {
            match results {
                serde_json::Value::Array(rows) if !rows.is_empty() => {
                    // Check if rows are arrays (rel results) or scalars
                    if let Some(serde_json::Value::Array(_)) = rows.first() {
                        self.print_rel_results(rows);
                    } else {
                        self.print_coll_results(rows);
                    }
                }
                _ => self.pretty_print_json(json),
            }
        }
    }

    fn print_rel_results(&self, rows: &[serde_json::Value]) {
        let stdout = io::stdout();
        let mut tw = TabWriter::new(stdout.lock());

        for row in rows {
            if let serde_json::Value::Array(cols) = row {
                for col in cols {
                    let _ = write!(tw, "| {}\t", format_json_value(col));
                }
                let _ = writeln!(tw, "|");
            }
        }

        let _ = writeln!(
            tw,
            "({} row{})",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        );
        let _ = tw.flush();
    }

    fn print_coll_results(&self, values: &[serde_json::Value]) {
        let stdout = io::stdout();
        let mut tw = TabWriter::new(stdout.lock());

        for val in values {
            let _ = writeln!(tw, "| {}\t|", format_json_value(val));
        }

        let _ = writeln!(
            tw,
            "({} row{})",
            values.len(),
            if values.len() == 1 { "" } else { "s" }
        );
        let _ = tw.flush();
    }

    fn pretty_print_json(&self, json: &serde_json::Value) {
        match serde_json::to_string_pretty(json) {
            Ok(s) => println!("{s}"),
            Err(_) => println!("{json}"),
        }
    }

    fn print_success(&self, msg: &str) {
        eprintln!(
            "{green}{msg}{reset}",
            green = color::Fg(GREEN),
            msg = msg,
            reset = color::Fg(color::Reset)
        );
    }

    fn print_error(&self, msg: &str) {
        eprintln!(
            "{red}{msg}{reset}",
            red = color::Fg(RED),
            msg = msg,
            reset = color::Fg(color::Reset)
        );
    }

    fn print_timing(&self, duration: std::time::Duration) {
        let micros = duration.as_micros();
        let time_str = if micros < 1_000 {
            format!("{}us", micros)
        } else if micros < 1_000_000 {
            format!("{:.2}ms", micros as f64 / 1000.0)
        } else {
            format!("{:.2}s", duration.as_secs_f64())
        };
        eprintln!(
            "{bold}Time: {time}{reset}",
            bold = style::Bold,
            time = time_str,
            reset = style::Reset
        );
    }

    // -- Input helpers --

    fn is_tty(&self) -> bool {
        self.interface.is_some()
    }

    fn read_line(&mut self, prompt: &str) -> LineResult {
        match self.interface {
            Some(ref iface) => {
                iface.set_prompt(prompt).unwrap_or(());
                match iface.read_line() {
                    Ok(ReadResult::Input(s)) => LineResult::Input(s),
                    Ok(ReadResult::Signal(Signal::Interrupt)) => LineResult::Interrupt,
                    _ => LineResult::Eof,
                }
            }
            None => {
                eprint!("{prompt}");
                if io::stderr().flush().is_err() {
                    return LineResult::Eof;
                }
                let mut s = String::new();
                match io::stdin().read_line(&mut s) {
                    Ok(0) | Err(_) => LineResult::Eof,
                    Ok(_) => {
                        if s.ends_with('\n') {
                            s.truncate(s.len() - 1);
                        }
                        if s.ends_with('\r') {
                            s.truncate(s.len() - 1);
                        }
                        LineResult::Input(s)
                    }
                }
            }
        }
    }

    fn add_history(&self, line: &str) {
        if let Some(ref iface) = self.interface {
            iface.add_history(line.to_string());
        }
    }

    fn save_history(&self) {
        if let Some(ref iface) = self.interface {
            let p = history_file_path();
            let _ = iface.save_history(&p);
        }
    }
}

enum LineResult {
    Input(String),
    Interrupt,
    Eof,
}

/// Load all schema attribute idents from the database for tab completion.
fn load_schema_idents(client: &mut Client, completer: &MentatCompleter) {
    match client.query("SELECT ident FROM mentat.schema ORDER BY ident", &[]) {
        Ok(rows) => {
            let idents: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
            completer.set_schema_idents(idents);
        }
        Err(_) => {
            // Schema may not exist yet; ignore silently
        }
    }
}

/// Try to extract a displayable string from a postgres row column.
fn get_column_display(
    row: &postgres::Row,
    idx: usize,
    col: &postgres::Column,
) -> String {
    // Try common types in order
    let type_name = col.type_().name();
    match type_name {
        "int2" | "int4" | "int8" => row
            .try_get::<_, i64>(idx)
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "NULL".to_string()),
        "float4" | "float8" | "numeric" => row
            .try_get::<_, f64>(idx)
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "NULL".to_string()),
        "bool" => row
            .try_get::<_, bool>(idx)
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "NULL".to_string()),
        "text" | "varchar" | "bpchar" | "name" => row
            .try_get::<_, String>(idx)
            .unwrap_or_else(|_| "NULL".to_string()),
        "jsonb" | "json" => row
            .try_get::<_, serde_json::Value>(idx)
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "NULL".to_string()),
        _ => {
            // Fall back to trying as string
            row.try_get::<_, String>(idx)
                .unwrap_or_else(|_| format!("<{type_name}>"))
        }
    }
}

fn format_json_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "nil".to_string(),
        other => other.to_string(),
    }
}
