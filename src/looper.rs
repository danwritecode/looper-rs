use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tera::{Tera, Context};

use crate::{
    services::{
        ChatHandler,
        handlers::anthropic_non_streaming::AnthropicNonStreamingHandler,
        handlers::openai_completions_non_streaming::OpenAINonStreamingChatHandler,
    },
    tools::LooperTools,
    types::{Handlers, turn::TurnResult},
};

pub struct Looper {
    handler: Box<dyn ChatHandler>,
    message_history: Option<Value>,
    tools: Option<Arc<dyn LooperTools>>,
}

pub struct LooperBuilder<'a> {
    handler_type: Handlers<'a>,
    message_history: Option<Value>,
    tools: Option<Arc<dyn LooperTools>>,
    instructions: Option<String>,
}

impl<'a> LooperBuilder<'a> {
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

    pub fn build(self) -> Result<Looper> {
        let handler: Box<dyn ChatHandler> = match self.handler_type {
            Handlers::Anthropic(m) => {
                let mut handler = AnthropicNonStreamingHandler::new(
                    &m,
                    &get_system_message(self.instructions.as_deref())?,
                )?;

                if let Some(t) = &self.tools {
                    handler.set_tools(t.get_tools());
                }

                Box::new(handler)
            }
            Handlers::OpenAICompletions(m) => {
                let mut handler = OpenAINonStreamingChatHandler::new(
                    &m,
                    &get_system_message(self.instructions.as_deref())?,
                )?;

                if let Some(t) = &self.tools {
                    handler.set_tools(t.get_tools());
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
