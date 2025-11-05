// Debug Command - CLI tool for sending commands to debug runtime
//
// This tool provides an ergonomic command-line interface for interacting
// with the debug runtime's HTTP API, enabling easy testing and debugging
// of the game from scripts and LLMs.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "debug_command")]
#[command(about = "CLI tool for controlling debug runtime")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Debug runtime host URL
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    host: String,

    /// Output raw JSON instead of pretty-printed
    #[arg(long)]
    raw: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Test connectivity to debug runtime
    Health,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Health) => {
            println!("Health check placeholder - implementation coming in Phase 1.4");
            println!("Would connect to: {}", cli.host);
        }
        None => {
            println!("Debug command placeholder - implementation coming in Phase 1.4");
            println!("Usage: cargo dbgc health");
        }
    }

    Ok(())
}
