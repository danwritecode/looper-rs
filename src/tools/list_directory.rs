use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{tools::LooperTool, types::LooperToolDefinition};

#[derive(Default)]
pub struct ListDirectoryTool;

#[async_trait]
impl LooperTool for ListDirectoryTool {
    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .name("list_directory")
            .description("List files and directories at the given path. Returns names with '/' suffix for directories.")
            .paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The directory path to list (default: current directory)" }
                },
                "required": []
            }))
    }

    async fn execute(&self, args: &Value) -> Value {
        let path = args["path"].as_str().unwrap_or(".");
        match tokio::fs::read_dir(path).await {
            Ok(mut entries) => {
                let mut items = Vec::new();
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false);
                    if is_dir {
                        items.push(format!("{}/", name));
                    } else {
                        items.push(name);
                    }
                }
                items.sort();
                json!({ "path": path, "entries": items })
            }
            Err(e) => json!({ "error": format!("Failed to list {}: {}", path, e) }),
        }
    }
}
