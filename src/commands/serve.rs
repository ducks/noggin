use crate::mcp::NogginServer;
use anyhow::{bail, Result};
use rmcp::ServiceExt;
use std::env;

pub async fn serve_command() -> Result<()> {
    let repo_path = env::current_dir()?;
    let noggin_path = repo_path.join(".noggin");

    if !noggin_path.exists() {
        bail!("Not initialized. Run 'noggin init' first.");
    }

    let server = NogginServer::new(noggin_path);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
