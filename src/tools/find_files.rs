use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{tools::LooperTool, types::LooperToolDefinition};

#[derive(Default)]
pub struct FindFilesTool;

#[async_trait]
impl LooperTool for FindFilesTool {
    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("find_files")
            .set_description("Find files matching a glob pattern recursively. Returns a list of matching file paths.")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern to match, e.g. '**/*.rs', 'src/**/*.toml'" },
                    "path": { "type": "string", "description": "The root directory to search from (default: current directory)" }
                },
                "required": ["pattern"]
            }))
    }

    async fn execute(&self, args: &Value) -> Value {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        let path = args["path"].as_str().unwrap_or(".");
        let output = tokio::process::Command::new("find")
            .args([path, "-path", pattern, "-type", "f"])
            .output()
            .await;
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let files: Vec<&str> = stdout.lines().take(200).collect();
                json!({ "pattern": pattern, "path": path, "files": files })
            }
            Err(e) => json!({ "error": format!("find failed: {}", e) }),
        }
    }
}
