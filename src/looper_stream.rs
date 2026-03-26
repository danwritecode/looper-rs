use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use crate::{
    looper::Looper,
    services::{
        StreamingChatHandler, anthropic::AnthropicHandler, gemini::GeminiHandler,
        openai_completions::OpenAIChatHandler, openai_responses::OpenAIResponsesHandler,
    },
    tools::{EmptyToolSet, LooperTools, SubAgentTool},
    types::{HandlerToLooperMessage, Handlers, LooperToInterfaceMessage, MessageHistory},
};
use anyhow::Result;
use tera::{Context, Tera};
use tokio::sync::mpsc::{self, Sender};

const BUFFER_DRAIN_INTERVAL_MS: u64 = 5;

pub struct LooperStream {
    handler: Box<dyn StreamingChatHandler>,
    message_history: Option<MessageHistory>,
    tools: Arc<dyn LooperTools>,
}

pub struct LooperStreamBuilder<'a> {
    handler_type: Handlers<'a>,
    message_history: Option<MessageHistory>,
    tools: Option<Box<dyn LooperTools>>,
    instructions: Option<String>,
    interface_sender: Option<Sender<LooperToInterfaceMessage>>,
    sub_agent: Option<Looper>,
    buffered_output: bool,
}

impl<'a> LooperStreamBuilder<'a> {
    pub fn message_history(mut self, history: MessageHistory) -> Self {
        self.message_history = Some(history);
        self
    }

    pub fn tools(mut self, tools: Box<dyn LooperTools>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Sub Agent MUST receive a Looper instance with the *SAME* Tools
    ///
    /// This is currently a limitation that cannot be enforced a type level.
    /// The main agent loop is expecting the Sub Agent to have the same tools
    /// that it has!
    pub fn sub_agent(mut self, looper: Looper) -> Self {
        self.sub_agent = Some(looper);
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

    pub fn buffered_output(mut self) -> Self {
        self.buffered_output = true;
        self
    }

    pub async fn build(mut self) -> Result<LooperStream> {
        let sub_agent_enabled = self.sub_agent.is_some();
        let (handler_looper_sender, mut handler_looper_receiver) = mpsc::channel(10000);

        let handler: Box<dyn StreamingChatHandler> = match self.handler_type {
            Handlers::OpenAICompletions(m) => {
                let mut handler = OpenAIChatHandler::new(
                    handler_looper_sender,
                    m,
                    &get_system_message(self.instructions.as_deref(), sub_agent_enabled)?,
                )?;

                if let Some(t) = self.tools.as_mut() {
                    if let Some(sa) = self.sub_agent {
                        let agent_tools = Arc::new(SubAgentTool::new(sa));
                        let _ = t.add_tool(agent_tools).await;
                    }
                    handler.set_tools(t.get_tools().await);
                }

                Box::new(handler)
            }
            Handlers::OpenAIResponses(m) => {
                let mut handler = OpenAIResponsesHandler::new(
                    handler_looper_sender,
                    m,
                    &get_system_message(self.instructions.as_deref(), sub_agent_enabled)?,
                )?;

                if let Some(t) = self.tools.as_mut() {
                    if let Some(sa) = self.sub_agent {
                        let agent_tools = Arc::new(SubAgentTool::new(sa));
                        let _ = t.add_tool(agent_tools).await;
                    }
                    handler.set_tools(t.get_tools().await);
                }

                Box::new(handler)
            }
            Handlers::Anthropic(m) => {
                let mut handler = AnthropicHandler::new(
                    handler_looper_sender,
                    m,
                    &get_system_message(self.instructions.as_deref(), sub_agent_enabled)?,
                )?;

                if let Some(t) = self.tools.as_mut() {
                    if let Some(sa) = self.sub_agent {
                        let agent_tools = Arc::new(SubAgentTool::new(sa));
                        let _ = t.add_tool(agent_tools).await;
                    }
                    handler.set_tools(t.get_tools().await);
                }

                Box::new(handler)
            }
            Handlers::Gemini(m) => {
                let mut handler = GeminiHandler::new(
                    handler_looper_sender,
                    m,
                    &get_system_message(self.instructions.as_deref(), sub_agent_enabled)?,
                )?;

                if let Some(t) = self.tools.as_mut() {
                    if let Some(sa) = self.sub_agent {
                        let agent_tools = Arc::new(SubAgentTool::new(sa));
                        let _ = t.add_tool(agent_tools).await;
                    }
                    handler.set_tools(t.get_tools().await);
                }

                Box::new(handler)
            }
        };

        // Spawn a single long-lived listener task that forwards messages
        // from the handler to the interface and executes tool calls.
        if let Some(l_i_s) = self.interface_sender {
            let buffered = self.buffered_output;
            tokio::spawn(async move {
                if buffered {
                    let mut pool: VecDeque<char> = VecDeque::new();
                    let mut interval =
                        tokio::time::interval(Duration::from_millis(BUFFER_DRAIN_INTERVAL_MS));
                    let mut channel_open = true;
                    loop {
                        tokio::select! {
                            biased;
                            msg = handler_looper_receiver.recv(), if channel_open => {
                                match msg {
                                    Some(HandlerToLooperMessage::Assistant(m)) => {
                                        pool.extend(m.chars());
                                    }
                                    Some(other) => {
                                        if drain_pool(&l_i_s, &mut pool).await.is_err() { break; }
                                        if forward_non_text(&l_i_s, other).await.is_err() { break; }
                                    }
                                    None => {
                                        channel_open = false;
                                    }
                                }
                            }
                            _ = interval.tick() => {
                                if let Some(c) = pool.pop_front() {
                                    if l_i_s.send(LooperToInterfaceMessage::Assistant(c.to_string())).await.is_err() { break; }
                                } else if !channel_open {
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    while let Some(message) = handler_looper_receiver.recv().await {
                        match message {
                            HandlerToLooperMessage::Assistant(m) => {
                                if l_i_s
                                    .send(LooperToInterfaceMessage::Assistant(m))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            other => {
                                if forward_non_text(&l_i_s, other).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        }

        match self.tools {
            Some(t) => Ok(LooperStream {
                handler,
                message_history: self.message_history,
                tools: Arc::from(t),
            }),
            None => Ok(LooperStream {
                handler,
                message_history: self.message_history,
                tools: Arc::new(EmptyToolSet),
            }),
        }
    }
}

impl LooperStream {
    pub fn builder(handler_type: Handlers) -> LooperStreamBuilder {
        LooperStreamBuilder {
            handler_type,
            message_history: None,
            tools: None,
            sub_agent: None,
            instructions: None,
            interface_sender: None,
            buffered_output: false,
        }
    }

    pub async fn send(&mut self, message: &str) -> Result<MessageHistory> {
        let history = self
            .handler
            .send_message(self.message_history.clone(), message, self.tools.clone())
            .await?;

        self.message_history = Some(history.clone());

        Ok(history)
    }
}

async fn drain_pool(
    sender: &Sender<LooperToInterfaceMessage>,
    pool: &mut VecDeque<char>,
) -> Result<()> {
    let text: String = pool.drain(..).collect();
    if !text.is_empty() {
        sender
            .send(LooperToInterfaceMessage::Assistant(text))
            .await?;
    }
    Ok(())
}

async fn forward_non_text(
    sender: &Sender<LooperToInterfaceMessage>,
    msg: HandlerToLooperMessage,
) -> Result<()> {
    let interface_msg = match msg {
        HandlerToLooperMessage::Assistant(_) => unreachable!("Assistant handled separately"),
        HandlerToLooperMessage::Thinking(m) => LooperToInterfaceMessage::Thinking(m),
        HandlerToLooperMessage::ThinkingComplete => LooperToInterfaceMessage::ThinkingComplete,
        HandlerToLooperMessage::ToolCallPending(id) => {
            LooperToInterfaceMessage::ToolCallPending(id)
        }
        HandlerToLooperMessage::ToolCallRequest(tc) => {
            LooperToInterfaceMessage::ToolCall(tc.name.clone())
        }
        HandlerToLooperMessage::ToolCallComplete(id) => {
            LooperToInterfaceMessage::ToolCallComplete(id)
        }
        HandlerToLooperMessage::TurnComplete => LooperToInterfaceMessage::TurnComplete,
    };
    sender.send(interface_msg).await?;
    Ok(())
}

fn render_system_message(
    template: &str,
    instructions: Option<&str>,
    sub_agent_enabled: bool,
) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("system_prompt", template)?;

    let mut ctx = Context::new();
    if let Some(inst) = instructions {
        ctx.insert("instructions", inst);
    }

    if sub_agent_enabled {
        ctx.insert("sub_agent", &true);
    }

    Ok(tera.render("system_prompt", &ctx)?)
}

fn get_system_message(instructions: Option<&str>, sub_agent_enabled: bool) -> Result<String> {
    render_system_message(
        include_str!("../prompts/system_prompt.txt"),
        instructions,
        sub_agent_enabled,
    )
}
