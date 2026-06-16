mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "workflow")]
#[command(about = "Agnostic Workflow/Trigger System CLI")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate rules in a directory or file
    Validate {
        /// Path to rules directory or file
        path: String,
    },
    /// Run sidecar `*.test.flow` tests under a path
    Test {
        /// Path to a test file or directory containing tests
        path: String,

        /// Substring filter: only run tests whose name contains this
        #[arg(long)]
        filter: Option<String>,

        /// Emit a machine-readable JSON report instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Evaluate rules against an event
    Evaluate {
        /// Path to rules directory or file
        path: String,

        /// Event name to fire
        #[arg(short, long)]
        event: String,

        /// Event data as JSON string
        #[arg(short, long)]
        data: Option<String>,

        /// Variables as JSON string
        #[arg(short, long)]
        vars: Option<String>,
    },
    /// Export rules between formats
    Export {
        /// Input file path
        input: String,

        /// Output file path
        #[arg(short, long)]
        output: String,
    },
    /// Watch directory for changes and evaluate events
    Watch {
        /// Path to rules directory
        path: String,

        /// Event name to fire on changes
        #[arg(short, long)]
        event: String,

        /// Event data as JSON string
        #[arg(short, long)]
        data: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let exit_code: Result<i32, String> = match cli.command {
        Commands::Validate { path } => commands::validate::run(&path)
            .map(|_| 0)
            .map_err(|e| e.to_string()),
        Commands::Test { path, filter, json } => {
            // The test subcommand has its own pass/fail exit
            // code (0 on success, 1 on any failure), distinct
            // from the error path below.
            match commands::test_runner::run(&path, filter.as_deref(), json) {
                Ok(code) if code == std::process::ExitCode::SUCCESS => std::process::exit(0),
                Ok(_) => std::process::exit(1),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Evaluate {
            path,
            event,
            data,
            vars,
        } => commands::evaluate::run(&path, &event, data.as_deref(), vars.as_deref())
            .await
            .map(|_| 0)
            .map_err(|e| e.to_string()),
        Commands::Export { input, output } => commands::export::run(&input, &output)
            .map(|_| 0)
            .map_err(|e| e.to_string()),
        Commands::Watch { path, event, data } => {
            commands::watch::run(&path, &event, data.as_deref())
                .await
                .map(|_| 0)
                .map_err(|e| e.to_string())
        }
    };

    match exit_code {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
