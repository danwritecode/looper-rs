use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{tools::LooperTool, types::LooperToolDefinition};

#[derive(Default)]
pub struct ReadFileTool;

#[async_trait]
impl LooperTool for ReadFileTool {
    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .name("read_file")
            .description("Read the contents of a file at a given path. Returns the file contents as a string.")
            .paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file path to read (absolute or relative to cwd)" }
                },
                "required": ["path"]
            }))
    }

    async fn execute(&self, args: &Value) -> Value {
        let path = args["path"].as_str().unwrap_or("");
        match tokio::fs::read_to_string(path).await {
            Ok(content) => json!({ "path": path, "content": content }),
            Err(e) => json!({ "error": format!("Failed to read {}: {}", path, e) }),
        }
    }
}

