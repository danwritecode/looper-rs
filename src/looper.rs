use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc::{self, Sender}};
use crate::{services::{ChatHandler, OpenAIChatHandler}, types::{HandlerToLooperMessage, LooperToHandlerMessage, LooperToHandlerToolCallResult, LooperToInterfaceMessage}};


#[derive(Debug)]
pub enum LooperState {
    Continue(String),
    Done
}

pub struct Looper {
    sender: Sender<LooperToInterfaceMessage>,
}

impl Looper {
    pub fn new(sender: Sender<LooperToInterfaceMessage>) -> Self {
        Looper { 
            sender
        }
    }

    pub async fn send(&mut self, message: &str) -> Result<()> {
        let (h_l_tx, mut h_l_rx) = mpsc::channel(10000); // for handler to send messages to looper
        let (l_h_tx, mut l_h_rx) = mpsc::channel(10000); // for looper to send messages to handler

        let system_message = get_system_message();
        let mut handler = OpenAIChatHandler::new(h_l_tx, l_h_rx, &system_message)?;
        let sender = self.sender.clone();

        tokio::spawn(async move{
            while let Some(message) = h_l_rx.recv().await { match message { 
                    HandlerToLooperMessage::Assistant(m) => {
                        sender.send(LooperToInterfaceMessage::Assistant(m)).await.unwrap();
                    },
                    HandlerToLooperMessage::ToolCallRequest(tc) => {
                        sender.send(LooperToInterfaceMessage::ToolCall(tc.name.clone())).await.unwrap();

                        let response = match tc.name.as_ref() {
                            "read_file" => read_file(&tc.args).await,
                            "write_file" => write_file(&tc.args).await,
                            "list_directory" => list_directory(&tc.args).await, "grep" => grep(&tc.args).await,
                            "find_files" => find_files(&tc.args).await,
                            _ => json!({"error": format!("Unknown function: {}", tc.name)}),
                        };
                        
                        let tc_result = LooperToHandlerToolCallResult {
                            id: tc.id,
                            value: response
                        };

                        l_h_tx.send(LooperToHandlerMessage::ToolCallResult(tc_result)).await.unwrap();
                    }
                }
            }
        });

        handler.send_message(message).await?;

        // agent loop TODO
        // loop {
        // }

        Ok(())
    }
}

// async fn get_agent_loop_state(looper_state: Arc<Mutex<LooperState>>) -> serde_json::Value {
//     let looper_state_lock = looper_state.lock().await;
//
//     match &*looper_state_lock {
//         LooperState::Continue(c) => {
//             json!({ "state": format!("Agent Loop State is 'continue' with value: '{}'", c) })
//         },
//         LooperState::Done => {
//             json!({ "state": "Agent Loop State is 'done'" })
//         },
//         _ => json!({ "error": "Unsupported state type | Supported enum values: 'done' and 'continue'" })
//     }
//      
// }

// async fn set_agent_loop_state(args: &str, looper_state: Arc<Mutex<LooperState>>) -> serde_json::Value {
//     let args: serde_json::Value = args.parse().unwrap_or(json!({}));
//     let state = args["state"].as_str().unwrap_or("");
//     let continue_reason = args["continue_reason"].as_str().unwrap_or("No continue reason provided");
//     let mut looper_state_lock = looper_state.lock().await;
//
//     match state {
//         "continue" => {
//             *looper_state_lock = LooperState::Continue(continue_reason.to_string());
//             json!({ "response": "Set looper state to Continue" })
//         },
//         "done" => {
//             *looper_state_lock = LooperState::Done;
//             json!({ "response": "Set looper state to Done" })
//         },
//         _ => json!({ "error": "Unsupported state type | Supported enum values: 'done' and 'continue'" })
//     }
//      
// }

async fn read_file(args: &Value) -> serde_json::Value {
    let path = args["path"].as_str().unwrap_or("");
    match tokio::fs::read_to_string(path).await {
        Ok(content) => json!({ "path": path, "content": content }),
        Err(e) => json!({ "error": format!("Failed to read {}: {}", path, e) }),
    }
}

async fn write_file(args: &Value) -> serde_json::Value {
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

async fn list_directory(args: &Value) -> serde_json::Value {
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

async fn grep(args: &Value) -> serde_json::Value {
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

async fn find_files(args: &Value) -> serde_json::Value {
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


fn get_system_message() -> String {
    format!("
        # Agent Loop System Prompt
        You are an AI assistant with access to tools. Use them proactively to complete tasks.

        ## Core Loop Behavior
        You are in a loop that by default *continues*. This means after you respond, you will be re-invoked automatically. Use this to work incrementally — interleaving is your default mode of operation. Do not batch all your work silently and respond once at the end. Work incrementally: act, report, act, report.

        <example>
        Good: Read file A → tell user what you found → read file B → tell user what you found → done
        Bad: Read file A, read file B, read file C → dump everything on the user at once → done
        </example>

        You have one loop control tool:
        - `set_agent_loop_state` — call with `'done'` when you're finished, or `'continue'` with a reason when you have more work. **You must call this every turn.** If you don't set done, you'll be re-invoked.

        That's it. Don't overthink the loop. Focus on the task. Use your tools, tell the user what you found, and keep going until the work is done.

        Loop Rules:
            1. After each tool call or a *related batch of tool calls* you MUST send an assistant message summarizing what you just learned/did and what you'll do next if you plan to continue.
            2. Once you are finished, set the loop state to 'done' and give a final message to the user before handing back control.
            3. For simple greetings or inquiries that do not require tool use, just respond and set done immediately.

        General Rules:
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
