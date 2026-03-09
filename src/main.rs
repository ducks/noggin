use clap::{Parser, Subcommand};
use colored::Colorize;
use llm_noggin::commands::init::init_command;
use llm_noggin::commands::learn::learn_command;
use llm_noggin::commands::serve::serve_command;
use llm_noggin::commands::status::status_command;
use llm_noggin::git::walker::{walk_commits, WalkOptions};
use llm_noggin::query::{QueryEngine, QueryOptions};
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

        /// Force full analysis (ignore manifest, re-analyze everything)
        #[arg(long)]
        full: bool,
    },

    /// Query the knowledge base
    Ask {
        /// Question to ask about the codebase
        query: String,

        /// Maximum number of results (default 10)
        #[arg(long, default_value = "10")]
        max_results: usize,

        /// Filter by category (decisions, patterns, bugs, migrations, facts)
        #[arg(long)]
        category: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Start MCP server for tool integration
    Serve,

    /// Show what's scanned and what's pending
    Status {
        /// Show detailed file and commit listings
        #[arg(long, short)]
        verbose: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => init_command(),
        Commands::Learn { verify, full } => learn_command(full, verify).await,
        Commands::Ask { query, max_results, category, json } => {
            let repo_path = env::current_dir()?;
            let noggin_path = repo_path.join(".noggin");

            if !noggin_path.exists() {
                anyhow::bail!("Not initialized. Run 'noggin init' first.");
            }

            let engine = QueryEngine::new(noggin_path);
            let opts = QueryOptions {
                max_results,
                category,
            };

            let results = engine.search(&query, &opts)?;

            if results.is_empty() {
                if json {
                    println!("[]");
                } else {
                    println!("No results for \"{}\"", query);
                    println!("Try a broader query or run {} to learn more.", "'noggin learn'".cyan());
                }
                return Ok(());
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
                return Ok(());
            }

            println!("{} results for \"{}\"\n", results.len(), query);

            let mut current_category = String::new();
            for result in &results {
                if result.category != current_category {
                    current_category = result.category.clone();
                    println!("{}", current_category.to_uppercase().bold());
                }
                println!("  {} {}", result.file_path.dimmed(), format!("[{}]", result.matched_fields.join(", ")).dimmed());
                println!("  {}", result.what.cyan());
                println!("  {}", result.why);
                println!();
            }

            Ok(())
        }
        Commands::Serve => serve_command().await,
        Commands::Status { verbose, json } => status_command(verbose, json),
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
                    println!(
                        "    {} files changed, {} insertions(+), {} deletions(-)",
                        commit.files_changed, commit.insertions, commit.deletions
                    );
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
