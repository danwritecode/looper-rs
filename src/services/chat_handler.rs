use std::sync::Arc;

use async_openai::{Client, config::OpenAIConfig, types::chat::{ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestDeveloperMessageArgs, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessage, ChatCompletionRequestUserMessageArgs, ChatCompletionTool, ChatCompletionTools, CreateChatCompletionRequestArgs, FinishReason, FunctionObjectArgs, ReasoningEffort}};

use async_trait::async_trait;
use async_recursion::async_recursion;

use anyhow::Result;
use serde_json::json;
use tokio::sync::{Mutex, mpsc::Sender};
use futures::StreamExt;

use crate::looper::{LooperResponse, LooperState};

#[async_trait]
pub trait ChatHandler {
    fn add_message(&mut self, message: &str) -> Result<()>;
    async fn send_message(&mut self) -> Result<()>;
}

pub struct OpenAIChatHandler {
    client: Client<OpenAIConfig>, 
    messages: Vec<ChatCompletionRequestMessage>,
    sender: Sender<LooperResponse>,
    looper_state: Arc<Mutex<LooperState>>
}

impl OpenAIChatHandler {
    pub fn new(sender: Sender<LooperResponse>) -> Result<Self> {
        let client = Client::new();
        let message = ChatCompletionRequestSystemMessageArgs::default()
            .content(get_system_message())
            .build()?
            .into();

        let looper_state = Arc::new(Mutex::new(LooperState::Continue("".to_string())));

        Ok(OpenAIChatHandler { client, messages: vec![message], sender, looper_state })
    }

    #[async_recursion]
    async fn inner_send_message(&mut self) -> Result<String> {
        let request = CreateChatCompletionRequestArgs::default()
            .model("gpt-5.2")
            .max_completion_tokens(50000u32)
            .messages(self.messages.clone())
            .reasoning_effort(ReasoningEffort::Low)
            .tools(build_tools()?)
            .build()?;

        let mut stream = self.client.chat().create_stream(request).await?;
        let mut res_buf = Vec::new();
        let mut tool_calls = Vec::new();
        let mut execution_handles = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    for choice in response.choices.iter() {
                        if let Some(ref content) = choice.delta.content {
                            res_buf.push(content.clone());
                            let res = LooperResponse::Assistant(content.clone());
                            self.sender.send(res).await?;
                        }

                        // Collect tool call chunks
                        if let Some(tool_call_chunks) = &choice.delta.tool_calls {
                            for chunk in tool_call_chunks {
                                let index = chunk.index as usize;

                                // Ensure we have enough space in the vector
                                while tool_calls.len() <= index {
                                    tool_calls.push(ChatCompletionMessageToolCall {
                                        id: String::new(),
                                        function: Default::default(),
                                    });
                                }

                                // Update the tool call with chunk data
                                let tool_call = &mut tool_calls[index];
                                if let Some(id) = &chunk.id {
                                    tool_call.id = id.clone();
                                }
                                if let Some(function_chunk) = &chunk.function {
                                    if let Some(name) = &function_chunk.name {
                                        tool_call.function.name = name.to_string();
                                    }
                                    if let Some(arguments) = &function_chunk.arguments {
                                        tool_call.function.arguments.push_str(&arguments);
                                    }
                                }
                            }
                        }

                        // When tool calls are complete, start executing them immediately
                        if matches!(choice.finish_reason, Some(FinishReason::ToolCalls)) {
                            // Spawn execution tasks for all collected tool calls
                            for tool_call in tool_calls.iter() {
                                let name = tool_call.function.name.clone();

                                let res = LooperResponse::ToolCall(name.clone());
                                self.sender.send(res).await?;

                                let args = tool_call.function.arguments.clone();
                                let tool_call_id = tool_call.id.clone();
                                let looper_state_ptr = self.looper_state.clone();

                                let handle = tokio::spawn(async move {
                                    let result = call_function(&name, &args, looper_state_ptr).await;
                                    (tool_call_id, result)
                                });
                                execution_handles.push(handle);
                            }
                        }
                    }
                }
                Err(err) => {
                    println!("error: {err:?}");
                }
            }
        }

        // Wait for all tool call executions to complete (outside the stream loop)
        if !execution_handles.is_empty() {
            let mut tool_responses = Vec::new();
            for handle in execution_handles {
                let (tool_call_id, response) = handle.await?;
                tool_responses.push((tool_call_id, response));
            }

            // Add assistant message with tool calls
            let assistant_tool_calls: Vec<ChatCompletionMessageToolCalls> = tool_calls
                .iter()
                .map(|tc| tc.clone().into()) // From<ChatCompletionMessageToolCall>
                .collect();

            self.messages.push(
                ChatCompletionRequestAssistantMessage {
                    content: None,
                    tool_calls: Some(assistant_tool_calls),
                    ..Default::default()
                }
                .into(),
            );

            // Add tool response messages
            for (tool_call_id, response) in tool_responses {
                self.messages.push(
                    ChatCompletionRequestToolMessage {
                        content: response.to_string().into(),
                        tool_call_id,
                    }
                    .into(),
                );
            }

            return self.inner_send_message().await;
        }

        let response = res_buf.join("");

        Ok(response)
    }
}

#[async_trait]
impl ChatHandler for OpenAIChatHandler {
    fn add_message(&mut self, message: &str) -> Result<()> {
        let message = ChatCompletionRequestUserMessageArgs::default()
            .content(message)
            .build()?
            .into();

        // handler
        self.messages.push(message);

        Ok(())
    }

    async fn send_message(&mut self) -> Result<()> {
        let mut looper_state_lock = self.looper_state.lock().await;
        *looper_state_lock = LooperState::Continue("".to_string());
        drop(looper_state_lock);


        loop {
            let looper_state_lock = self.looper_state.lock().await;

            if matches!(*looper_state_lock, LooperState::Done) {
                break;
            }

            drop(looper_state_lock);

            let response = self.inner_send_message().await?;

            let message = ChatCompletionRequestAssistantMessageArgs::default()
                .content(response.clone())
                .build()?
                .into();

            self.messages.push(message);
        }

        let end_res = LooperResponse::Assistant("<END>".to_string());
        self.sender.send(end_res).await?;

        Ok(())
    }
}


fn build_tools() -> Result<Vec<ChatCompletionTools>> {
    Ok(vec![
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("get_agent_loop_state")
                    .description("Allows you to get the current state of your loop so you can decide if you should continue with more tool calls or to set your loop to done and respond to the user.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {}
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("set_agent_loop_state")
                    .description("You will use this to signal to the agent loop when you want to continue or when to finish your turn. This means that you can choose to continue so that you have the opportunity to use more tools calls even after responding to a user.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "state": { "type": "string", "description": "An enum of either 'continue' or 'done'" },
                            "continue_reason": { "type": "string", "description": "If state == 'continue', then provide the continue reason which should be the work you want to accomplish in the next loop iteration." }
                        },
                        "required": ["state"]
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("read_file")
                    .description("Read the contents of a file at a given path. Returns the file contents as a string.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "The file path to read (absolute or relative to cwd)" }
                        },
                        "required": ["path"]
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("write_file")
                    .description("Write content to a file. Creates the file if it doesn't exist, overwrites if it does.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "The file path to write to" },
                            "content": { "type": "string", "description": "The content to write to the file" }
                        },
                        "required": ["path", "content"]
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("list_directory")
                    .description("List files and directories at the given path. Returns names with '/' suffix for directories.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "The directory path to list (default: current directory)" }
                        },
                        "required": []
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("grep")
                    .description("Search for a regex pattern in files. Recursively searches the given path and returns matching lines with file paths and line numbers.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string", "description": "The regex pattern to search for" },
                            "path": { "type": "string", "description": "The file or directory to search in (default: current directory)" }
                        },
                        "required": ["pattern"]
                    }))
                    .build()?,
            }
        ),
        // ChatCompletionTools::Function(
        //     ChatCompletionTool {
        //         function: FunctionObjectArgs::default()
        //             .name("run_command")
        //             .description("Execute a shell command and return its stdout and stderr. Use for running tests, builds, git commands, etc.")
        //             .parameters(json!({
        //                 "type": "object",
        //                 "properties": {
        //                     "command": { "type": "string", "description": "The shell command to execute" }
        //                 },
        //                 "required": ["command"]
        //             }))
        //             .build()?,
        //     }
        // ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("find_files")
                    .description("Find files matching a glob pattern recursively. Returns a list of matching file paths.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string", "description": "Glob pattern to match, e.g. '**/*.rs', 'src/**/*.toml'" },
                            "path": { "type": "string", "description": "The root directory to search from (default: current directory)" }
                        },
                        "required": ["pattern"]
                    }))
                    .build()?,
            }
        ),
        // ChatCompletionTools::Function(
        //     ChatCompletionTool {
        //         function: FunctionObjectArgs::default()
        //             .name("edit_file")
        //             .description("Replace an exact string in a file with new content. The old_string must match exactly (including whitespace/indentation). Use read_file first to see the current content.")
        //             .parameters(json!({
        //                 "type": "object",
        //                 "properties": {
        //                     "path": { "type": "string", "description": "The file path to edit" },
        //                     "old_string": { "type": "string", "description": "The exact string to find and replace" },
        //                     "new_string": { "type": "string", "description": "The string to replace it with" }
        //                 },
        //                 "required": ["path", "old_string", "new_string"]
        //             }))
        //             .build()?,
        //     }
        // ),
    ])
}

async fn call_function(name: &str, args: &str, looper_state: Arc<Mutex<LooperState>>) -> serde_json::Value {
    match name {
        "set_agent_loop_state" => set_agent_loop_state(args, looper_state).await,
        "get_agent_loop_state" => get_agent_loop_state(looper_state).await,
        "read_file" => read_file(args).await,
        "write_file" => write_file(args).await,
        "list_directory" => list_directory(args).await,
        "grep" => grep(args).await,
        // "run_command" => run_command(args).await,
        "find_files" => find_files(args).await,
        // "edit_file" => edit_file(args).await,
        _ => json!({"error": format!("Unknown function: {}", name)}),
    }
}

async fn get_agent_loop_state(looper_state: Arc<Mutex<LooperState>>) -> serde_json::Value {
    let looper_state_lock = looper_state.lock().await;

    match &*looper_state_lock {
        LooperState::Continue(c) => {
            json!({ "state": format!("Agent Loop State is 'continue' with value: '{}'", c) })
        },
        LooperState::Done => {
            json!({ "state": "Agent Loop State is 'done'" })
        },
        _ => json!({ "error": "Unsupported state type | Supported enum values: 'done' and 'continue'" })
    }
     
}

async fn set_agent_loop_state(args: &str, looper_state: Arc<Mutex<LooperState>>) -> serde_json::Value {
    let args: serde_json::Value = args.parse().unwrap_or(json!({}));
    let state = args["state"].as_str().unwrap_or("");
    let continue_reason = args["continue_reason"].as_str().unwrap_or("No continue reason provided");
    let mut looper_state_lock = looper_state.lock().await;

    match state {
        "continue" => {
            *looper_state_lock = LooperState::Continue(continue_reason.to_string());
            json!({ "response": "Set looper state to Continue" })
        },
        "done" => {
            *looper_state_lock = LooperState::Done;
            json!({ "response": "Set looper state to Done" })
        },
        _ => json!({ "error": "Unsupported state type | Supported enum values: 'done' and 'continue'" })
    }
     
}

async fn read_file(args: &str) -> serde_json::Value {
    let args: serde_json::Value = args.parse().unwrap_or(json!({}));
    let path = args["path"].as_str().unwrap_or("");
    match tokio::fs::read_to_string(path).await {
        Ok(content) => json!({ "path": path, "content": content }),
        Err(e) => json!({ "error": format!("Failed to read {}: {}", path, e) }),
    }
}

async fn write_file(args: &str) -> serde_json::Value {
    let args: serde_json::Value = args.parse().unwrap_or(json!({}));
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

async fn list_directory(args: &str) -> serde_json::Value {
    let args: serde_json::Value = args.parse().unwrap_or(json!({}));
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

async fn grep(args: &str) -> serde_json::Value {
    let args: serde_json::Value = args.parse().unwrap_or(json!({}));
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

// async fn run_command(args: &str) -> serde_json::Value {
//     let args: serde_json::Value = args.parse().unwrap_or(json!({}));
//     let command = args["command"].as_str().unwrap_or("");
//     let output = tokio::process::Command::new("sh")
//         .args(["-c", command])
//         .output()
//         .await;
//     match output {
//         Ok(out) => {
//             let stdout = String::from_utf8_lossy(&out.stdout).to_string();
//             let stderr = String::from_utf8_lossy(&out.stderr).to_string();
//             json!({
//                 "command": command,
//                 "exit_code": out.status.code(),
//                 "stdout": stdout,
//                 "stderr": stderr
//             })
//         }
//         Err(e) => json!({ "error": format!("Failed to execute: {}", e) }),
//     }
// }

async fn find_files(args: &str) -> serde_json::Value {
    let args: serde_json::Value = args.parse().unwrap_or(json!({}));
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

// async fn edit_file(args: &str) -> serde_json::Value {
//     let args: serde_json::Value = args.parse().unwrap_or(json!({}));
//     let path = args["path"].as_str().unwrap_or("");
//     let old_string = args["old_string"].as_str().unwrap_or("");
//     let new_string = args["new_string"].as_str().unwrap_or("");
//     match tokio::fs::read_to_string(path).await {
//         Ok(content) => {
//             if !content.contains(old_string) {
//                 return json!({ "error": "old_string not found in file" });
//             }
//             let count = content.matches(old_string).count();
//             if count > 1 {
//                 return json!({ "error": format!("old_string is ambiguous, found {} occurrences. Provide more context to make it unique.", count) });
//             }
//             let new_content = content.replacen(old_string, new_string, 1);
//             match tokio::fs::write(path, &new_content).await {
//                 Ok(()) => json!({ "path": path, "status": "edited" }),
//                 Err(e) => json!({ "error": format!("Failed to write {}: {}", path, e) }),
//             }
//         }
//         Err(e) => json!({ "error": format!("Failed to read {}: {}", path, e) }),
//     }
// }

fn get_system_message() -> String {
    format!("
    # Agent Loop System Prompt

    You are an AI assistant with access to tools. Use them proactively to complete tasks.

    ## Core Loop Behavior

    **After each tool call (or small batch of tool calls), you MUST send an assistant message summarizing what you just learned/did and what
     you will do next. Do not just keep setting the loop state to continue without telling the user what you are doing next. then set `set_agent_loop_state(state='continue', continue_reason='...')` if more work remains.**

    - On every iteration you must call 'get_agent_loop_state'. This tells you what state you're in and whether you should continue or not with more tool calls.
    - With 'set_agent_loop_state' this allows you to control the control flow of the program. You can use this to call tools -> respond to user -> call tools -> respond to user.
        - You MUST set this to 'done' when you are done, otherwise the loop will continue forever.
    - When given a task, **break it into steps** before starting. Track your progress explicitly.
    - **Use tools liberally.** Search, read, execute, and verify rather than guessing or assuming.
    - You can use more than one tool call at once! don't hesistate to chain multiple together.
    - After every tool call, **assess what you learned** and decide your next action. Do not stop after a single tool call unless the task is fully complete.
    - When you are done, **always respond to the user** with a concise summary of what you did and the outcome. Never end on a tool call with no follow-up message.

    ## Task Execution

    1. **Plan** — Identify what needs to happen. List concrete steps.
    2. **Act** — Execute steps one at a time using available tools. Batch independent tool calls in parallel when possible.
    3. **Verify** — After implementing, confirm correctness (run tests, check output, re-read files). Do not assume success.
    4. **Report** — Summarize the result to the user. Be concise and direct.

    ## Tool Usage Policy

    - Prefer tools over assumptions. If you can look something up, look it up.
    - When multiple independent pieces of information are needed, make tool calls in parallel.
    - If a tool call fails, adjust your approach and retry rather than giving up.
    - Do not invent information that a tool could provide.

    ## Style

    - Be concise. Do the work, report the result.
    - Do not narrate your thought process unless asked. Skip preamble and postamble.
    - If you cannot complete a task, say so clearly and explain what's blocking you.
    ")
}
