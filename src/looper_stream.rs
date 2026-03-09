use std::sync::Arc;

use crate::{
    services::{StreamingChatHandler, anthropic::AnthropicHandler, openai_completions::OpenAIChatHandler},
    tools::LooperTools,
    types::{HandlerToLooperMessage, Handlers, LooperToHandlerToolCallResult, LooperToInterfaceMessage},
};
use anyhow::Result;
use serde_json::{Value, json};
use tera::{Tera, Context};
use tokio::sync::mpsc::{self, Sender};

pub struct LooperStream {
    handler: Box<dyn StreamingChatHandler>,
    message_history: Option<Value>,
}

pub struct LooperStreamBuilder<'a> {
    handler_type: Handlers<'a>,
    message_history: Option<Value>,
    tools: Option<Arc<dyn LooperTools>>,
    instructions: Option<String>,
    interface_sender: Option<Sender<LooperToInterfaceMessage>>,
}

impl<'a> LooperStreamBuilder<'a> {
    pub fn message_history(mut self, history: Value) -> Self {
        self.message_history = Some(history);
        self
    }

    pub fn tools(mut self, tools: Arc<dyn LooperTools>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn interface_sender(mut self, sender: Sender<LooperToInterfaceMessage>) -> Self {
        self.interface_sender = Some(sender);
        self
    }

    pub fn build(self) -> Result<LooperStream> {
        let (handler_looper_sender, mut handler_looper_receiver) = mpsc::channel(10000);

        let handler: Box<dyn StreamingChatHandler> = match self.handler_type {
            Handlers::OpenAICompletions(m) => {
                let mut handler = OpenAIChatHandler::new(
                    handler_looper_sender,
                    &m,
                    &get_system_message(self.instructions.as_deref())?
                )?;

                if let Some(t) = &self.tools {
                    handler.set_tools(t.get_tools());
                }

                Box::new(handler)
            },
            Handlers::Anthropic(m) => {
                let mut handler = AnthropicHandler::new(
                    handler_looper_sender,
                    &m,
                    &get_system_message(self.instructions.as_deref())?
                )?;

                if let Some(t) = &self.tools {
                    handler.set_tools(t.get_tools());
                }

                Box::new(handler)
            }
        };

        // Spawn a single long-lived listener task that forwards messages
        // from the handler to the interface and executes tool calls.
        if let Some(l_i_s) = self.interface_sender {
            let tools_clone = self.tools.clone();
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

                            let response = match &tools_clone {
                                Some(t) => t.run_tool(&tc.name, tc.args).await,
                                None => json!({"Error": "Unsupported tool called"})
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
        }

        Ok(LooperStream {
            handler,
            message_history: self.message_history,
        })
    }
}

impl LooperStream {
    pub fn builder(handler_type: Handlers) -> LooperStreamBuilder {
        LooperStreamBuilder {
            handler_type,
            message_history: None,
            tools: None,
            instructions: None,
            interface_sender: None,
        }
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

fn get_system_message(instructions: Option<&str>) -> Result<String> {
    render_system_message(include_str!("../prompts/system_prompt.txt"), instructions)
}
