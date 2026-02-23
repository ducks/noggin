use clap::{Parser, Subcommand};
use llm_noggin::commands::init::init_command;
use llm_noggin::git::walker::{walk_commits, WalkOptions};
use std::env;

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
    
    /// Walk git commits and display metadata (debug)
    GitWalk {
        /// Start from specific commit hash
        #[arg(long)]
        since: Option<String>,
        
        /// Limit number of commits to show
        #[arg(long)]
        limit: Option<usize>,
        
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Init => init_command(),
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
        Commands::GitWalk { since, limit, json } => {
            let repo_path = env::current_dir()?;
            let options = WalkOptions {
                since_commit: since,
                limit,
                ..Default::default()
            };
            
            let result = walk_commits(&repo_path, options)?;
            
            if json {
                println!("{}", serde_json::to_string_pretty(&result.commits)?);
            } else {
                println!("Commits ({})", result.commits.len());
                println!();
                for commit in &result.commits {
                    println!("commit {}", commit.hash);
                    println!("Author: {}", commit.author);
                    println!("Date:   {}", commit.timestamp);
                    println!();
                    println!("    {}", commit.message_summary);
                    println!();
                    println!("    {} files changed, {} insertions(+), {} deletions(-)",
                        commit.files_changed, commit.insertions, commit.deletions);
                    println!();
                }
                
                if let Some(next_hash) = result.next_hash {
                    println!("More commits available. Resume with: --since {}", next_hash);
                }
            }
            
            Ok(())
        }
    }
}
