use std::{
    collections::HashMap,
    error::Error,
    io::{self, Write},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use console::{Style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify, mpsc};

use looper::{
    looper::Looper,
    looper_stream::LooperStream,
    tools::{LooperTool, LooperTools},
    types::{Handlers, LooperToInterfaceMessage, LooperToolDefinition},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let term = Term::stdout();
    term.clear_screen()?;
    let theme = Theme::default();

    let tools: Box<dyn LooperTools> = Box::new(ToolSet::new());
    let agent_tools: Box<dyn LooperTools> = Box::new(ToolSet::new());

    let (tx, mut rx) = mpsc::channel(10000);

    // NOTE: For now, agent_looper doesn't need to stream tokens since the user
    // doesn't directly see it's token stream anyway. Might as well just leave it
    // as non-streaming, unless there is obviously value to changing this.
    let agent_looper = Looper::builder(Handlers::Gemini("gemini-3-flash-preview"))
        .tools(agent_tools)
        .instructions("
            You are an agent researching specific tasks for another agent that is invoking you.
            Report back with concise and clear findings since the agent invoking you will rely on this information.
        ")
        .build().await?;

    let mut looper = LooperStream::builder(Handlers::Gemini("gemini-3-flash-preview"))
        .sub_agent(agent_looper)
        .tools(tools)
        .interface_sender(tx)
        .instructions("You're being used as a CLI example for an agent loop. Be succinct yet friendly and helpful.")
        .buffered_output()
        .build().await?;

    let turn_done = Arc::new(Notify::new());
    let turn_done_tx = turn_done.clone();

    tokio::spawn(async move {
        let theme = Theme::default();
        let mut spinner: Option<ProgressBar> = None;

        while let Some(message) = rx.recv().await {
            if let Some(sp) = spinner.take() {
                sp.finish_and_clear();
            }

            match message {
                LooperToInterfaceMessage::Assistant(m) => {
                    print!("{}", m);
                    io::stdout().flush().ok();
                }
                LooperToInterfaceMessage::Thinking(m) => {
                    print!("{}", theme.thinking.apply_to(&m));
                    io::stdout().flush().ok();
                }
                LooperToInterfaceMessage::ThinkingComplete => {
                    println!();
                }
                LooperToInterfaceMessage::ToolCall(name) => {
                    spinner = Some(theme.tool_spinner(&name));
                }
                LooperToInterfaceMessage::ToolCallPending(_id) => {
                    // TODO: Implement intelligent swap of tool calls based on id
                }
                LooperToInterfaceMessage::ToolCallComplete(_id) => {
                    // TODO: Handle tool call completion
                }
                LooperToInterfaceMessage::TurnComplete => {
                    println!("\n{}", theme.separator_line());
                    turn_done_tx.notify_one();
                }
            }
        }
    });

    loop {
        print!("{}", theme.prompt());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        looper.send(&input).await?;
        turn_done.notified().await;
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
            .set_description("Read the contents of a file at a given path. Returns the file contents as a string.")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file path to read (absolute or relative to cwd)" }
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

struct WriteFileTool;

#[async_trait]
impl LooperTool for WriteFileTool {
    fn get_tool_name(&self) -> String {
        "write_file".to_string()
    }

    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("write_file")
            .set_description("Write content to a file. Creates the file if it doesn't exist, overwrites if it does.")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file path to write to" },
                    "content": { "type": "string", "description": "The content to write to the file" }
                },
                "required": ["path", "content"]
            }))
    }

    async fn execute(&mut self, args: &Value) -> Value {
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

struct ListDirectoryTool;

#[async_trait]
impl LooperTool for ListDirectoryTool {
    fn get_tool_name(&self) -> String {
        "list_directory".to_string()
    }

    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("list_directory")
            .set_description("List files and directories at the given path. Returns names with '/' suffix for directories.")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The directory path to list (default: current directory)" }
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

struct GrepTool;

#[async_trait]
impl LooperTool for GrepTool {
    fn get_tool_name(&self) -> String {
        "grep".to_string()
    }

    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("grep")
            .set_description("Search for a regex pattern in files. Recursively searches the given path and returns matching lines with file paths and line numbers.")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "The regex pattern to search for" },
                    "path": { "type": "string", "description": "The file or directory to search in (default: current directory)" }
                },
                "required": ["pattern"]
            }))
    }

    async fn execute(&mut self, args: &Value) -> Value {
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
}

struct FindFilesTool;

#[async_trait]
impl LooperTool for FindFilesTool {
    fn get_tool_name(&self) -> String {
        "find_files".to_string()
    }

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

    async fn execute(&mut self, args: &Value) -> Value {
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

// ── Tool sets ────────────────────────────────────────────────────────

struct ToolSet {
    tools: HashMap<String, Mutex<Arc<dyn LooperTool>>>,
}

impl ToolSet {
    fn new() -> Self {
        let mut tools: HashMap<String, Mutex<Arc<dyn LooperTool>>> = HashMap::new();
        tools.insert("read_file".to_string(), Mutex::new(Arc::new(ReadFileTool)));
        tools.insert(
            "write_file".to_string(),
            Mutex::new(Arc::new(WriteFileTool)),
        );
        tools.insert(
            "list_directory".to_string(),
            Mutex::new(Arc::new(ListDirectoryTool)),
        );
        tools.insert("grep".to_string(), Mutex::new(Arc::new(GrepTool)));
        tools.insert(
            "find_files".to_string(),
            Mutex::new(Arc::new(FindFilesTool)),
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

// ── CLI STYLING ────────────────────────────────────────────────────────
struct Theme {
    thinking: Style,
    separator: Style,
    tool_spinner: Style,
    prompt: Style,
    #[allow(dead_code)]
    greeting: Style,
}

impl Theme {
    fn default() -> Self {
        Theme {
            thinking: Style::new().green().dim().italic(),
            separator: Style::new().green().dim(),
            tool_spinner: Style::new().yellow(),
            prompt: Style::new().green().bold(),
            greeting: Style::new().green().bold(),
        }
    }

    fn prompt(&self) -> String {
        self.prompt.apply_to("> ").to_string()
    }

    fn separator_line(&self) -> String {
        self.separator
            .apply_to("────────────────────────────────")
            .to_string()
    }

    fn tool_spinner(&self, name: &str) -> ProgressBar {
        let sp = ProgressBar::new_spinner();
        sp.set_style(
            ProgressStyle::default_spinner().tick_strings(&["▖", "▘", "▝", "▗", "▚", "▞", ""]),
        );
        sp.set_message(self.tool_spinner.apply_to(name).to_string());
        sp.enable_steady_tick(Duration::from_millis(80));
        sp
    }
}
