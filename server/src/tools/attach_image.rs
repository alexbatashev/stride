use std::sync::Arc;

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use minisql::ConnectionPool;
use serde_json::{Value as JsonValue, json};
use stride_agent::{AgentConfig, Tool, ToolDesc};
use uuid::Uuid;

use crate::api::images::publish_image;
use crate::vfs::{MountedVfs, Vfs};

/// Makes an image in the virtual file system viewable by the (vision-capable)
/// model. The image is published behind a public URL when one can be built,
/// otherwise embedded inline, and surfaced to the model via the `__images`
/// result convention handled by the agent core.
pub struct AttachImageTool {
    pub fs: MountedVfs,
    pub vfs: Arc<Vfs>,
    pub db: ConnectionPool,
    pub owner: Uuid,
    pub public_url: Option<String>,
}

#[derive(ToolDesc)]
struct AttachImageParams {
    /// Absolute path to an image file in the file system, e.g.
    /// "/home/agent/screenshot.png" or "/home/user/photos/diagram.jpg".
    path: String,
}

#[async_trait(?Send)]
impl Tool for AttachImageTool {
    fn name(&self) -> &str {
        "attach_image"
    }

    fn readable_name(&self) -> &str {
        "Attach image"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                name: self.name().to_owned(),
                description: "Attach an image file so you can see its contents. Use this for any \
                              image (screenshot, photo, diagram) the user refers to. The image is \
                              shown to you right after this call returns."
                    .to_string(),
                parameters: Some(AttachImageParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: JsonValue) -> JsonValue {
        let params = match AttachImageParams::decode(args) {
            Ok(p) => p,
            Err(e) => return json!({ "error": e }),
        };

        let (bytes, mime) = match self.fs.read_bytes(&params.path).await {
            Ok(data) => data,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        if !mime.as_deref().is_some_and(|m| m.starts_with("image/")) {
            return json!({ "error": format!("{} is not an image", params.path) });
        }

        let image = match publish_image(
            &self.vfs,
            &self.db,
            self.owner,
            self.public_url.as_deref(),
            &bytes,
            mime.as_deref(),
        )
        .await
        {
            Ok(image) => image,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        json!({
            "success": true,
            "path": params.path,
            "__images": [image],
        })
    }
}
