pub mod read_file;
pub mod write_file;
pub mod list_directory;
pub mod grep;
pub mod find_files;

pub use read_file::*;
pub use write_file::*;
pub use list_directory::*;
pub use grep::*;
pub use find_files::*;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::types::LooperToolDefinition;

#[async_trait]
pub trait LooperTool {
    async fn execute(&self, args: &Value) -> Value;
    fn tool(&self) -> LooperToolDefinition;
}

pub struct LooperTools {
    read_file: ReadFileTool,
    write_file: WriteFileTool,
    list_directory: ListDirectoryTool,
    grep: GrepTool,
    find_files: FindFilesTool
}

impl LooperTools {
    pub fn new() -> Self {
        let read_file = ReadFileTool::default(); 
        let write_file = WriteFileTool::default(); 
        let list_directory = ListDirectoryTool::default(); 
        let grep = GrepTool::default(); 
        let find_files = FindFilesTool::default(); 

        LooperTools { 
            read_file,
            write_file,
            list_directory,
            grep,
            find_files
        }
    }

    pub fn get_tools(&self) -> Vec<LooperToolDefinition> {
        vec![
            self.read_file.tool(),
            self.write_file.tool(),
            self.list_directory.tool(),
            self.grep.tool(),
            self.find_files.tool(),
        ]
    }

    pub async fn run_tool(&self, name: &str, args: Value) -> Value {
        match name {
            "read_file" => self.read_file.execute(&args).await,
            "write_file" => self.write_file.execute(&args).await,
            "list_directory" => self.list_directory.execute(&args).await,
            "grep" => self.grep.execute(&args).await,
            "find_files" => self.find_files.execute(&args).await,
            _ => json!({"error": format!("Unknown function: {}", name)})
        }
    }
}

