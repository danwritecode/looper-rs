use std::sync::Arc;

use anyhow::Result;
use tera::{Tera, Context};
use tokio::sync::Mutex;

use crate::{
    services::{
        handlers::{anthropic_non_streaming::AnthropicNonStreamingHandler, openai_completions_non_streaming::OpenAINonStreamingChatHandler, openai_responses_non_streaming::OpenAIResponsesNonStreamingHandler}, ChatHandler
    },
    tools::{LooperTools, SubAgentTool},
    types::{turn::TurnResult, Handlers, MessageHistory},
};

pub struct Looper {
    handler: Box<dyn ChatHandler>,
    message_history: Option<MessageHistory>,
    tools: Option<Arc<Mutex<dyn LooperTools>>>,
}

pub struct LooperBuilder<'a> {
    handler_type: Handlers<'a>,
    message_history: Option<MessageHistory>,
    tools: Option<Arc<Mutex<dyn LooperTools>>>,
    instructions: Option<String>,
    enable_sub_agents: bool
}

impl<'a> LooperBuilder<'a> {
    pub fn message_history(mut self, history: MessageHistory) -> Self {
        self.message_history = Some(history);
        self
    }

    pub fn tools(mut self, tools: Arc<Mutex<dyn LooperTools>>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn enable_sub_agents(mut self, enable: bool) -> Self {
        self.enable_sub_agents = enable;
        self
    }

    pub async fn build(mut self) -> Result<Looper> {
        let handler: Box<dyn ChatHandler> = match self.handler_type {
            Handlers::Anthropic(m) => {
                let mut handler = AnthropicNonStreamingHandler::new(
                    &m,
                    &get_system_message(self.instructions.as_deref())?,
                )?;

                if let Some(t) = self.tools.as_mut() {
                    let tc = t.clone();
                    let mut t = t.lock().await;
                    if self.enable_sub_agents {
                        let _ = t.add_tool(Arc::new(Mutex::new(SubAgentTool::new(tc)))).await;
                    }
                    handler.set_tools(t.get_tools().await);
                }

                Box::new(handler)
            }
            Handlers::OpenAICompletions(m) => {
                let mut handler = OpenAINonStreamingChatHandler::new(
                    &m,
                    &get_system_message(self.instructions.as_deref())?,
                )?;

                if let Some(t) = self.tools.as_mut() {
                    let tc = t.clone();
                    let mut t = t.lock().await;
                    if self.enable_sub_agents {
                        let _ = t.add_tool(Arc::new(Mutex::new(SubAgentTool::new(tc)))).await;
                    }
                    handler.set_tools(t.get_tools().await);
                }

                Box::new(handler)
            }
            Handlers::OpenAIResponses(m) => {
                let mut handler = OpenAIResponsesNonStreamingHandler::new(
                    &m,
                    &get_system_message(self.instructions.as_deref())?,
                )?;

                if let Some(t) = self.tools.as_mut() {
                    let tc = t.clone();
                    let mut t = t.lock().await;
                    if self.enable_sub_agents {
                        let _ = t.add_tool(Arc::new(Mutex::new(SubAgentTool::new(tc)))).await;
                    }
                    handler.set_tools(t.get_tools().await);
                }

                Box::new(handler)
            }
        };

        Ok(Looper {
            handler,
            message_history: self.message_history,
            tools: self.tools,
        })
    }
}

impl Looper {
    pub fn builder(handler_type: Handlers) -> LooperBuilder {
        LooperBuilder {
            handler_type,
            message_history: None,
            tools: None,
            instructions: None,
            enable_sub_agents: false, 
        }
    }

    pub async fn send(&mut self, message: &str) -> Result<TurnResult> {
        let result = self
            .handler
            .send_message(
                self.message_history.clone(),
                message,
                self.tools.as_ref(),
            )
            .await?;

        self.message_history = Some(result.message_history.clone());

        Ok(result)
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
