use std::{collections::HashMap, error::Error, io::{self, Write}, sync::Arc};

use async_trait::async_trait;
use serde_json::{Value, json};

use looper::{
    looper::Looper,
    tools::{LooperTool, LooperTools},
    types::{Handlers, LooperToolDefinition},
};
use tokio::sync::Mutex;


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    let tools: Arc<Mutex<dyn LooperTools>> = Arc::new(Mutex::new(ToolSet::new()));

    let mut looper = Looper::builder(Handlers::OpenAIResponses("gpt-5.4"))
        .tools(tools)
        .enable_sub_agents(true)
        .instructions("You're being used as a CLI example for an agent loop. Be succinct yet friendly and helpful.")
        .build().await?;

    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let result = looper.send(&input).await?;

        for (i, step) in result.steps.iter().enumerate() {
            if !step.thinking.is_empty() {
                println!("[thinking] ...");
            }

            for tc in &step.tool_calls {
                println!("[tool: {}] args={}", tc.name, tc.args);
                println!("[result] {}", tc.result);
            }

            if let Some(text) = &step.text {
                if result.steps.len() > 1 {
                    println!("[step {}] {}", i + 1, text);
                }
            }
        }

        if let Some(final_text) = &result.final_text {
            println!("{}", final_text);
        }

        println!("────────────────────────────────");
    }
}


// ── Tool implementations ────────────────────────────────────────────

struct ReadFileTool;

#[async_trait]
impl LooperTool for ReadFileTool {
    fn get_tool_name(&self) -> String { "read_file".to_string() }

    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("read_file")
            .set_description("Read the contents of a file at a given path.")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file path to read" }
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

struct ListDirectoryTool;

#[async_trait]
impl LooperTool for ListDirectoryTool {
    fn get_tool_name(&self) -> String { "list_directory".to_string() }

    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("list_directory")
            .set_description("List files and directories at the given path.")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The directory path to list" }
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

// ── Tool set ────────────────────────────────────────────────────────

struct ToolSet {
    tools: HashMap<String, Arc<Mutex<dyn LooperTool>>>,
}

impl ToolSet {
    fn new() -> Self {
        let mut tools: HashMap<String, Arc<Mutex<dyn LooperTool>>> = HashMap::new();
        tools.insert("read_file".to_string(), Arc::new(Mutex::new(ReadFileTool)));
        tools.insert("list_directory".to_string(), Arc::new(Mutex::new(ListDirectoryTool)));
        ToolSet { tools }
    }
}

#[async_trait]
impl LooperTools for ToolSet {
    async fn get_tools(&self) -> Vec<LooperToolDefinition> {
        let mut tools = Vec::with_capacity(self.tools.len());

        for t in self.tools.values() {
            let guard = t.lock().await;
            tools.push(guard.tool().clone());
        }

        tools
    }

    async fn add_tool(&mut self, tool: Arc<Mutex<dyn LooperTool>>) {
        let tool_name = tool.lock().await.get_tool_name();
        self.tools.insert(tool_name, tool);
    }

    async fn run_tool(&self, name: &str, args: Value) -> Value {
        match self.tools.get(name) {
            Some(tool) => {
                let tool = tool.lock().await;
                tool.execute(&args).await
            },
            None => json!({"error": format!("Unknown function: {}", name)}),
        }
    }
}
