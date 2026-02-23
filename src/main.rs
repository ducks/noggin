use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "noggin")]
#[command(about = "Your codebase's noggin - extract and query codebase knowledge", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize .noggin/ directory in current repository
    Init,
    
    /// Analyze codebase and generate/update knowledge base
    Learn {
        /// Verify manifest without overwriting
        #[arg(long)]
        verify: bool,
    },
    
    /// Query the knowledge base
    Ask {
        /// Question to ask about the codebase
        query: String,
    },
    
    /// Start MCP server for tool integration
    Serve,
    
    /// Show what's scanned and what's pending
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Init => {
            println!("[noggin init] Not implemented yet");
            Ok(())
        }
        Commands::Learn { verify } => {
            if verify {
                println!("[noggin learn --verify] Not implemented yet");
            } else {
                println!("[noggin learn] Not implemented yet");
            }
            Ok(())
        }
        Commands::Ask { query } => {
            println!("[noggin ask] Query: {}", query);
            println!("Not implemented yet");
            Ok(())
        }
        Commands::Serve => {
            println!("[noggin serve] Not implemented yet");
            Ok(())
        }
        Commands::Status => {
            println!("[noggin status] Not implemented yet");
            Ok(())
        }
    }
}
