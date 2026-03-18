use std::{
    collections::HashMap,
    error::Error,
    io::{self, Write},
    sync::Arc,
};

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

    let tools: Box<dyn LooperTools> = Box::new(ToolSet::new());
    let agent_tools: Box<dyn LooperTools> = Box::new(ToolSet::new());

    let agent_looper = Looper::builder(Handlers::OpenAIResponses("gpt-5.4"))
        .tools(agent_tools)
        .instructions("You're being used as a CLI example for an agent loop. Be succinct yet friendly and helpful.")
        .build().await?;

    let mut looper = Looper::builder(Handlers::OpenAIResponses("gpt-5.4"))
        .tools(tools)
        .sub_agent(agent_looper)
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
    fn get_tool_name(&self) -> String {
        "read_file".to_string()
    }

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

    async fn execute(&mut self, args: &Value) -> Value {
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
    fn get_tool_name(&self) -> String {
        "list_directory".to_string()
    }

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

    async fn execute(&mut self, args: &Value) -> Value {
        let path = args["path"].as_str().unwrap_or(".");
        match tokio::fs::read_dir(path).await {
            Ok(mut entries) => {
                let mut items = Vec::new();
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry
                        .file_type()
                        .await
                        .map(|ft| ft.is_dir())
                        .unwrap_or(false);
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
    tools: HashMap<String, Mutex<Arc<dyn LooperTool>>>,
}

impl ToolSet {
    fn new() -> Self {
        let mut tools: HashMap<String, Mutex<Arc<dyn LooperTool>>> = HashMap::new();
        tools.insert("read_file".to_string(), Mutex::new(Arc::new(ReadFileTool)));
        tools.insert(
            "list_directory".to_string(),
            Mutex::new(Arc::new(ListDirectoryTool)),
        );
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

    async fn add_tool(&mut self, tool: Arc<dyn LooperTool>) {
        let tool_name = tool.get_tool_name();
        self.tools.insert(tool_name, Mutex::new(tool));
    }

    async fn run_tool(&self, name: String, args: Value) -> Value {
        match self.tools.get(&name) {
            Some(tool_mutex) => {
                let mut arc = tool_mutex.lock().await;
                let tool = Arc::get_mut(&mut arc).expect("tool has multiple references");
                tool.execute(&args).await
            }
            None => json!({"error": format!("Unknown function: {}", name)}),
        }
    }
}
