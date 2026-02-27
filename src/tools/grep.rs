use serde_json::{Value, json};

use crate::types::LooperTool;

pub fn tool() -> LooperTool {
    LooperTool::default()
        .name("grep")
        .description("Search for a regex pattern in files. Recursively searches the given path and returns matching lines with file paths and line numbers.")
        .paramters(json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "The regex pattern to search for" },
                "path": { "type": "string", "description": "The file or directory to search in (default: current directory)" }
            },
            "required": ["pattern"]
        }))
}

pub async fn execute(args: &Value) -> Value {
    let pattern = args["pattern"].as_str().unwrap_or("");
    let path = args["path"].as_str().unwrap_or(".");
    let output = tokio::process::Command::new("grep")
        .args(["-rn", "--include=*", pattern, path])
        .output()
        .await;
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<&str> = stdout.lines().take(100).collect();
            let truncated = stdout.lines().count() > 100;
            json!({
                "pattern": pattern,
                "path": path,
                "matches": lines,
                "truncated": truncated
            })
        }
        Err(e) => json!({ "error": format!("grep failed: {}", e) }),
    }
}
