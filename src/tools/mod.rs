pub mod read_file;
pub mod write_file;
pub mod list_directory;
pub mod grep;
pub mod find_files;

use std::collections::HashMap;

pub use read_file::*;
pub use write_file::*;
pub use list_directory::*;
pub use grep::*;
pub use find_files::*;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::types::LooperToolDefinition;

#[async_trait]
pub trait LooperTool: Send + Sync {
    async fn execute(&self, args: &Value) -> Value;
    fn tool(&self) -> LooperToolDefinition;
}

pub struct LooperTools {
    tools: HashMap<String, Box<dyn LooperTool>>
}

impl LooperTools {
    pub fn new() -> Self {
        let mut tools: HashMap<String, Box<dyn LooperTool>> = HashMap::new();

        tools.insert("read_file".to_string(), Box::new(ReadFileTool));
        tools.insert("write_file".to_string(), Box::new(ReadFileTool));
        tools.insert("list_directory".to_string(), Box::new(ReadFileTool));
        tools.insert("grep".to_string(), Box::new(ReadFileTool));
        tools.insert("find_files".to_string(), Box::new(ReadFileTool));

        LooperTools { 
            tools
        }
    }

    pub fn get_tools(&self) -> Vec<LooperToolDefinition> {
        self.tools.values().into_iter().map(|t| t.tool()).collect::<Vec<LooperToolDefinition>>()
    }

    pub async fn run_tool(&self, name: &str, args: Value) -> Value {
        match self.tools.get(name) {
            Some(tool) => tool.execute(&args).await,
            None => json!({"error": format!("Unknown function: {}", name)})
        } 
    }
}

