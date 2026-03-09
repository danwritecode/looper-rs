use std::sync::Arc;

use crate::{
    services::{ChatHandler, anthropic::AnthropicHandler, openai_completions::OpenAIChatHandler},
    tools::{LooperTool, LooperTools, SetAgentLoopStateTool},
    types::{HandlerToLooperMessage, Handlers, LooperToHandlerToolCallResult, LooperToInterfaceMessage},
};
use anyhow::Result;
use serde_json::{Value, json};
use tera::{Tera, Context};
use tokio::sync::mpsc::{self, Sender};

pub struct Looper {
    handler: Box<dyn ChatHandler>,
    message_history: Option<Value>
}

#[derive(Debug)]
pub enum AgentLoopState {
    Continue(String),
    Done
}

impl Looper {
    pub fn new(
        handler_type: Handlers,
        message_history: Option<Value>,
        tools: Option<Arc<dyn LooperTools>>,
        instructions: Option<String>,
        looper_interface_sender: Sender<LooperToInterfaceMessage>
    ) -> Result<Self> {
        let (handler_looper_sender, mut handler_looper_receiver) = mpsc::channel(10000);

        let handler: Box<dyn ChatHandler> = match handler_type {
            Handlers::OpenAICompletions(m) => {
                let mut handler = OpenAIChatHandler::new(
                    handler_looper_sender,
                    &m,
                    &get_openai_system_message(instructions.as_deref())?
                )?;

                if let Some(t) = &tools {
                    let mut tool_defs = t.get_tools();
                    let set_agent_loop_state = SetAgentLoopStateTool;
                    tool_defs.push(set_agent_loop_state.tool());
                    handler.set_tools(tool_defs);
                }

                Box::new(handler)
            },
            Handlers::Anthropic(m) => {
                let mut handler = AnthropicHandler::new(
                    handler_looper_sender,
                    &m,
                    &get_anthropic_system_message(instructions.as_deref())?
                )?;

                if let Some(t) = &tools {
                    handler.set_tools(t.get_tools());
                }


                Box::new(handler)
            }
        };

        // Spawn a single long-lived listener task that forwards messages
        // from the handler to the interface and executes tool calls.
        let l_i_s = looper_interface_sender;
        let tools_clone = tools.clone();
        tokio::spawn(async move {
            while let Some(message) = handler_looper_receiver.recv().await {
                match message {
                    HandlerToLooperMessage::Assistant(m) => {
                        l_i_s
                            .send(LooperToInterfaceMessage::Assistant(m))
                            .await
                            .unwrap();
                    }
                    HandlerToLooperMessage::Thinking(m) => {
                        l_i_s
                            .send(LooperToInterfaceMessage::Thinking(m))
                            .await
                            .unwrap();
                    }
                    HandlerToLooperMessage::ThinkingComplete => {
                        l_i_s
                            .send(LooperToInterfaceMessage::ThinkingComplete)
                            .await
                            .unwrap();
                    }
                    HandlerToLooperMessage::ToolCallPending(index) => {
                        l_i_s
                            .send(LooperToInterfaceMessage::ToolCallPending(index))
                            .await
                            .unwrap();
                    }
                    HandlerToLooperMessage::ToolCallRequest(tc) => {
                        l_i_s
                            .send(LooperToInterfaceMessage::ToolCall(tc.name.clone()))
                            .await
                            .unwrap();

                        let response = if tc.name == "set_agent_loop_state" {
                            SetAgentLoopStateTool.execute(&tc.args).await
                        } else {
                            match &tools_clone {
                                Some(t) => t.run_tool(&tc.name, tc.args).await,
                                None => json!({"Error": "Unsupported tool called"})
                            }
                        };

                        let tc_result = LooperToHandlerToolCallResult {
                            id: tc.id,
                            value: response,
                        };

                        tc.tool_result_channel.send(tc_result).unwrap();
                    }
                    HandlerToLooperMessage::TurnComplete => {
                        l_i_s
                            .send(LooperToInterfaceMessage::TurnComplete)
                            .await
                            .unwrap();
                    }
                }
            }
        });

        Ok(Looper {
            handler,
            message_history,
        })
    }

    pub async fn send(&mut self, message: &str) -> Result<Value> {
        let messages = self.handler.send_message(self.message_history.clone(), message).await?;

        Ok(messages)
    }
}

fn render_system_message(template: &str, instructions: Option<&str>) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("system_prompt", template)?;

    let mut ctx = Context::new();
    if let Some(inst) = instructions {
        ctx.insert("instructions", inst);
    }

    Ok(tera.render("system_prompt", &ctx)?)
}

fn get_openai_system_message(instructions: Option<&str>) -> Result<String> {
    render_system_message(include_str!("../prompts/system_prompt_openai.txt"), instructions)
}

fn get_anthropic_system_message(instructions: Option<&str>) -> Result<String> {
    render_system_message(include_str!("../prompts/system_prompt_anthropic.txt"), instructions)
}
