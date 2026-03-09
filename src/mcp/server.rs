use crate::arf::ArfFile;
use crate::query::{QueryEngine, QueryOptions};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Clone)]
pub struct NogginServer {
    noggin_path: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryParams {
    /// Search query string
    pub query: String,
    /// Filter by category (decisions, patterns, bugs, migrations, facts)
    pub category: Option<String>,
    /// Maximum number of results (default 10)
    pub max_results: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetArfParams {
    /// Category directory (decisions, patterns, bugs, migrations, facts)
    pub category: String,
    /// ARF file name (without .arf extension)
    pub name: String,
}

#[tool_router]
impl NogginServer {
    pub fn new(noggin_path: PathBuf) -> Self {
        Self {
            noggin_path,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Search the noggin knowledge base for codebase knowledge matching a query. Returns ranked results from ARF files containing architectural decisions, code patterns, bug fixes, migrations, and facts.")]
    async fn query_knowledge(
        &self,
        params: Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let params = params.0;
        let engine = QueryEngine::new(self.noggin_path.clone());
        let opts = QueryOptions {
            max_results: params.max_results.unwrap_or(10),
            category: params.category,
        };

        let results = engine
            .search(&params.query, &opts)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No results for \"{}\"",
                params.query
            ))]));
        }

        let mut output = String::new();
        for result in &results {
            output.push_str(&format!(
                "[{}] {}\n  What: {}\n  Why: {}\n  How: {}\n\n",
                result.category, result.file_path, result.what, result.why, result.how,
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Read a specific ARF (Augmented Reasoning Format) file from the knowledge base. Provide the category (decisions, patterns, bugs, migrations, facts) and the file name (without .arf extension).")]
    async fn get_arf(
        &self,
        params: Parameters<GetArfParams>,
    ) -> Result<CallToolResult, McpError> {
        let params = params.0;
        let path = self
            .noggin_path
            .join(&params.category)
            .join(format!("{}.arf", params.name));

        if !path.exists() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "ARF file not found: {}/{}.arf",
                params.category, params.name
            ))]));
        }

        let arf = ArfFile::from_toml(&path)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut output = format!(
            "What: {}\nWhy: {}\nHow: {}",
            arf.what, arf.why, arf.how
        );

        if !arf.context.files.is_empty() {
            output.push_str(&format!("\nFiles: {}", arf.context.files.join(", ")));
        }
        if !arf.context.commits.is_empty() {
            output.push_str(&format!("\nCommits: {}", arf.context.commits.join(", ")));
        }
        if !arf.context.dependencies.is_empty() {
            output.push_str(&format!(
                "\nDependencies: {}",
                arf.context.dependencies.join(", ")
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "List all categories in the noggin knowledge base with the number of ARF files in each. Categories include decisions, patterns, bugs, migrations, and facts.")]
    async fn list_categories(&self) -> Result<CallToolResult, McpError> {
        let categories = ["decisions", "patterns", "bugs", "migrations", "facts"];
        let mut output = String::new();

        for category in &categories {
            let dir = self.noggin_path.join(category);
            let count = if dir.exists() {
                WalkDir::new(&dir)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map(|ext| ext == "arf").unwrap_or(false))
                    .count()
            } else {
                0
            };
            output.push_str(&format!("{}: {} files\n", category, count));
        }

        let other_count = WalkDir::new(&self.noggin_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();
                path.extension().map(|ext| ext == "arf").unwrap_or(false)
                    && path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .map(|n| !categories.contains(&n))
                        .unwrap_or(false)
            })
            .count();

        if other_count > 0 {
            output.push_str(&format!("other: {} files\n", other_count));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
}

#[tool_handler]
impl ServerHandler for NogginServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Noggin knowledge base server. Query codebase architectural decisions, \
                 patterns, bugs, migrations, and facts extracted by multi-model LLM analysis."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
