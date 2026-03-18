use std::sync::Arc;

use anyhow::Result;
use tera::{Context, Tera};

use crate::{
    services::{
        ChatHandler,
        handlers::{
            anthropic_non_streaming::AnthropicNonStreamingHandler,
            gemini_non_streaming::GeminiNonStreamingHandler,
            openai_completions_non_streaming::OpenAINonStreamingChatHandler,
            openai_responses_non_streaming::OpenAIResponsesNonStreamingHandler,
        },
    },
    tools::{EmptyToolSet, LooperTools, SubAgentTool},
    types::{Handlers, MessageHistory, turn::TurnResult},
};

pub struct Looper {
    handler: Box<dyn ChatHandler>,
    message_history: Option<MessageHistory>,
    tools: Arc<dyn LooperTools>,
}

pub struct LooperBuilder<'a> {
    handler_type: Handlers<'a>,
    message_history: Option<MessageHistory>,
    tools: Option<Box<dyn LooperTools>>,
    instructions: Option<String>,
    sub_agent: Option<Looper>,
}

impl<'a> LooperBuilder<'a> {
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

    pub async fn build(mut self) -> Result<Looper> {
        let sub_agent_enabled = self.sub_agent.is_some();

        let handler: Box<dyn ChatHandler> = match self.handler_type {
            Handlers::Anthropic(m) => {
                let mut handler = AnthropicNonStreamingHandler::new(
                    &m,
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
            Handlers::OpenAICompletions(m) => {
                let mut handler = OpenAINonStreamingChatHandler::new(
                    &m,
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
                let mut handler = OpenAIResponsesNonStreamingHandler::new(
                    &m,
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
                let mut handler = GeminiNonStreamingHandler::new(
                    &m,
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

        match self.tools {
            Some(t) => Ok(Looper {
                handler,
                message_history: self.message_history,
                tools: Arc::from(t),
            }),
            None => Ok(Looper {
                handler,
                message_history: self.message_history,
                tools: Arc::new(EmptyToolSet),
            }),
        }
    }
}

impl Looper {
    pub fn builder(handler_type: Handlers) -> LooperBuilder {
        LooperBuilder {
            handler_type,
            message_history: None,
            tools: None,
            sub_agent: None,
            instructions: None,
        }
    }

    pub async fn send(&mut self, message: &str) -> Result<TurnResult> {
        let result = self
            .handler
            .send_message(self.message_history.clone(), message, self.tools.clone())
            .await?;

        self.message_history = Some(result.message_history.clone());

        Ok(result)
    }
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
