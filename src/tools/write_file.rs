use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{tools::LooperTool, types::LooperToolDefinition};

#[derive(Default)]
pub struct WriteFileTool;

#[async_trait]
impl LooperTool for WriteFileTool {
    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .name("write_file")
            .description("Write content to a file. Creates the file if it doesn't exist, overwrites if it does.")
            .paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file path to write to" },
                    "content": { "type": "string", "description": "The content to write to the file" }
                },
                "required": ["path", "content"]
            }))
    }

    async fn execute(&self, args: &Value) -> Value {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        match tokio::fs::write(path, content).await {
            Ok(()) => json!({ "path": path, "bytes_written": content.len() }),
            Err(e) => json!({ "error": format!("Failed to write {}: {}", path, e) }),
        }
    }
}
