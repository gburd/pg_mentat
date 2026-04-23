mod commands;
mod repl;

use clap::Parser;

use crate::commands::Command;
use crate::repl::Repl;

/// pg_mentat_cli - Interactive Datalog shell for PostgreSQL with pg_mentat
#[derive(Parser, Debug)]
#[command(name = "pg_mentat_cli", version, about)]
struct Args {
    /// PostgreSQL host
    #[arg(long, default_value = "localhost")]
    host: String,

    /// PostgreSQL port
    #[arg(long, short = 'p', default_value = "5432")]
    port: u16,

    /// Database name
    #[arg(long, short = 'd', default_value = "mentat")]
    database: String,

    /// PostgreSQL user
    #[arg(long, short = 'U', default_value = "postgres")]
    user: String,

    /// PostgreSQL password
    #[arg(long, short = 'W')]
    password: Option<String>,

    /// Full connection string (overrides individual options)
    #[arg(long, short = 'c')]
    conninfo: Option<String>,

    /// Execute a Datalog query and exit
    #[arg(long, short = 'q')]
    query: Option<String>,

    /// Execute a transaction and exit
    #[arg(long, short = 't')]
    transact: Option<String>,

    /// Execute raw SQL and exit
    #[arg(long)]
    sql: Option<String>,

    /// Disable TTY/readline support
    #[arg(long)]
    no_tty: bool,
}

fn main() {
    let args = Args::parse();

    let conninfo = args.conninfo.unwrap_or_else(|| {
        let mut parts = format!(
            "host={} port={} dbname={} user={}",
            args.host, args.port, args.database, args.user
        );
        if let Some(ref pw) = args.password {
            parts.push_str(&format!(" password={pw}"));
        }
        parts
    });

    let mut repl = match Repl::new(&conninfo, !args.no_tty) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    // Non-interactive mode: execute single command and exit
    if let Some(query) = args.query {
        repl.run_command(Command::Query(query));
        return;
    }
    if let Some(transact) = args.transact {
        repl.run_command(Command::Transact(transact));
        return;
    }
    if let Some(sql) = args.sql {
        repl.run_command(Command::Sql(sql));
        return;
    }

    // Interactive REPL
    repl.run();
}
