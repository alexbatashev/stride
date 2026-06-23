//! Per-user connection to the official, hosted GitHub MCP server.
//!
//! Once a user links their GitHub account (see [`crate::api::github`]), their
//! access token is decrypted and forwarded as a bearer credential to the GitHub
//! MCP server, and every advertised tool is exposed to the agent under the
//! `github` prefix.

use minisql::ConnectionPool;
use stride_agent::mcp::{self, McpTool};
use uuid::Uuid;

use crate::{crypto::SecretCipher, db::github_connections};

/// Streamable HTTP endpoint of the official hosted GitHub MCP server.
pub const DEFAULT_MCP_URL: &str = "https://api.githubcopilot.com/mcp/";

/// Everything a worker needs to attach a user's GitHub MCP tools: where the
/// server lives and the cipher that unseals stored access tokens.
#[derive(Clone)]
pub struct GitHubRuntime {
    pub mcp_url: String,
    pub cipher: SecretCipher,
}

/// Connect to the GitHub MCP server on behalf of `user`, returning one tool per
/// advertised capability. Yields an empty list when the user has not linked an
/// account, the token cannot be decrypted, or the server is unreachable.
pub async fn connect_user_github_mcp(
    db: &ConnectionPool,
    user: Uuid,
    runtime: &GitHubRuntime,
) -> Vec<McpTool> {
    let connection = match github_connections::select()
        .where_(github_connections::user_id.eq(user))
        .all(db)
        .await
    {
        Ok(rows) => rows.into_iter().next(),
        Err(error) => {
            tracing::warn!(%error, user_id = %user, "failed to load GitHub connection");
            return Vec::new();
        }
    };
    let Some(connection) = connection else {
        return Vec::new();
    };

    let token = match runtime
        .cipher
        .decrypt(connection.id, &connection.access_token)
    {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(%error, user_id = %user, "failed to decrypt GitHub access token");
            return Vec::new();
        }
    };

    let server = mcp::McpServer {
        url: runtime.mcp_url.clone(),
        headers: vec![("Authorization".to_string(), format!("Bearer {token}"))],
    };
    match mcp::connect("github", server).await {
        Ok(tools) => {
            tracing::info!(
                user_id = %user,
                count = tools.len(),
                "connected to GitHub MCP server"
            );
            tools
        }
        Err(error) => {
            tracing::warn!(%error, user_id = %user, "failed to connect to GitHub MCP server");
            Vec::new()
        }
    }
}
