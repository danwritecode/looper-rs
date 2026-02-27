use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use tokio::sync::{Mutex, mpsc::{self, Receiver, Sender}};
use crate::{services::{ChatHandler, OpenAIChatHandler}, tools::{self, LooperTools}, types::{HandlerToLooperMessage, LooperToHandlerToolCallResult, LooperToInterfaceMessage}};

pub struct Looper {
    handler: Box<dyn ChatHandler>,
    looper_interface_sender: Sender<LooperToInterfaceMessage>,
    handler_looper_receiver: Arc<Mutex<Receiver<HandlerToLooperMessage>>>,
}

impl Looper {
    pub fn new(looper_interface_sender: Sender<LooperToInterfaceMessage>) -> Result<Self> {
        let (handler_looper_sender, handler_looper_receiver) = mpsc::channel(10000); // for handler to send messages to looper
        let handler_looper_receiver = Arc::new(Mutex::new(handler_looper_receiver));

        let system_message = get_system_message();

        // TODO: Pass in a provider enum and dynamically create handler
        // For now this is unneccessary as we only have 1 supported provider
        let mut handler = Box::new(OpenAIChatHandler::new(handler_looper_sender, &system_message)?);

        // get and set available tools
        let tools = LooperTools::new().get_tools();
        handler.set_tools(tools);

        Ok(Looper { 
            handler,
            looper_interface_sender,
            handler_looper_receiver
        })
    }

    pub async fn send(&mut self, message: &str) -> Result<()> {
        let l_i_s = self.looper_interface_sender.clone();
        let h_l_r = self.handler_looper_receiver.clone();

        let tools = LooperTools::new();

        tokio::spawn(async move{
            let mut h_l_r = h_l_r.lock().await;
            while let Some(message) = h_l_r.recv().await { match message { 
                    HandlerToLooperMessage::Assistant(m) => {
                        l_i_s.send(LooperToInterfaceMessage::Assistant(m)).await.unwrap();
                    },
                    HandlerToLooperMessage::ToolCallRequest(tc) => {
                        l_i_s.send(LooperToInterfaceMessage::ToolCall(tc.name.clone())).await.unwrap();

                        let response = tools.run_tool(&tc.name, tc.args).await;
                        
                        let tc_result = LooperToHandlerToolCallResult {
                            id: tc.id,
                            value: response
                        };

                        tc.tool_result_channel.send(tc_result).unwrap();
                    },
                    HandlerToLooperMessage::TurnComplete => {
                        l_i_s.send(LooperToInterfaceMessage::TurnComplete).await.unwrap();
                    }
                }
            }
        });

        self.handler.send_message(message).await?;

        // agent loop TODO
        // loop {
        // }

        Ok(())
    }
}


fn get_system_message() -> String {
    format!("you're a friendly assistant")
}

// fn get_system_message() -> String {
//     format!("
//         # Agent Loop System Prompt
//         You are an AI assistant with access to tools. Use them proactively to complete tasks.
//
//         ## Core Loop Behavior
//         You are in a loop that by default *continues*. This means after you respond, you will be re-invoked automatically. Use this to work incrementally — interleaving is your default mode of operation. Do not batch all your work silently and respond once at the end. Work incrementally: act, report, act, report.
//
//         <example>
//         Good: Read file A → tell user what you found → read file B → tell user what you found → done
//         Bad: Read file A, read file B, read file C → dump everything on the user at once → done
//         </example>
//
//         You have one loop control tool:
//         - `set_agent_loop_state` — call with `'done'` when you're finished, or `'continue'` with a reason when you have more work. **You must call this every turn.** If you don't set done, you'll be re-invoked.
//
//         That's it. Don't overthink the loop. Focus on the task. Use your tools, tell the user what you found, and keep going until the work is done.
//
//         Loop Rules:
//             1. After each tool call or a *related batch of tool calls* you MUST send an assistant message summarizing what you just learned/did and what you'll do next if you plan to continue.
//             2. Once you are finished, set the loop state to 'done' and give a final message to the user before handing back control.
//             3. For simple greetings or inquiries that do not require tool use, just respond and set done immediately.
//
//         General Rules:
//         - When given a task, **break it into steps** before starting. Track your progress explicitly.
//         - **Use tools liberally.** Search, read, execute, and verify rather than guessing or assuming.
//         - You can use more than one tool call at once! don't hesistate to chain multiple together.
//         - After every tool call, **assess what you learned** and decide your next action. Do not stop after a single tool call unless the task is fully complete.
//         - When you are done, **always respond to the user** with a concise summary of what you did and the outcome. Never end on a tool call with no follow-up message.
//
//         ## Task Execution
//         1. **Plan** — Identify what needs to happen. List concrete steps.
//         2. **Act** — Execute steps one at a time using available tools. Batch independent tool calls in parallel when possible.
//         3. **Verify** — After implementing, confirm correctness (run tests, check output, re-read files). Do not assume success.
//         4. **Report** — Summarize the result to the user. Be concise and direct.
//
//         ## Tool Usage Policy
//         - Prefer tools over assumptions. If you can look something up, look it up.
//         - When multiple independent pieces of information are needed, make tool calls in parallel.
//         - If a tool call fails, adjust your approach and retry rather than giving up.
//         - Do not invent information that a tool could provide.
//
//         ## Style
//         - Be concise. Do the work, report the result.
//         - Do not narrate your thought process unless asked. Skip preamble and postamble.
//         - If you cannot complete a task, say so clearly and explain what's blocking you.
//     ")
// }
