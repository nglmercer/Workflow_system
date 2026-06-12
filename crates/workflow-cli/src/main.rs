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

    let result = match cli.command {
        Commands::Validate { path } => commands::validate::run(&path),
        Commands::Evaluate {
            path,
            event,
            data,
            vars,
        } => commands::evaluate::run(&path, &event, data.as_deref(), vars.as_deref()).await,
        Commands::Export { input, output } => commands::export::run(&input, &output),
        Commands::Watch { path, event, data } => {
            commands::watch::run(&path, &event, data.as_deref()).await
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
