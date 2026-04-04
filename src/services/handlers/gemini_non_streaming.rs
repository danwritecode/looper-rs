use std::sync::Arc;

use gemini_rust::{Content, FunctionResponse, Gemini, Message, Model, Part, Role, Tool};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use tokio::task::JoinSet;

use crate::{
    mapping::tools::gemini::to_gemini_tool,
    services::ChatHandler,
    tools::LooperTools,
    types::{
        LooperToolDefinition, MessageHistory,
        turn::{ThinkingBlock, ToolCallRecord, TurnResult, TurnStep},
    },
};

pub struct GeminiNonStreamingHandler {
    client: Gemini,
    system_message: String,
    messages: Vec<Message>,
    tool: Option<Tool>,
}

impl GeminiNonStreamingHandler {
    pub fn new(model: &str, system_message: &str) -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .map_err(|_| {
                anyhow::anyhow!("GEMINI_API_KEY or GOOGLE_API_KEY environment variable must be set")
            })?;

        let model_id = if model.starts_with("models/") {
            Model::Custom(model.to_string())
        } else {
            Model::Custom(format!("models/{}", model))
        };
        let client = Gemini::with_model(&api_key, model_id)?;

        Ok(GeminiNonStreamingHandler {
            client,
            system_message: system_message.to_string(),
            messages: vec![],
            tool: None,
        })
    }

    #[async_recursion]
    async fn inner_send_message(
        &mut self,
        tools_runner: Arc<dyn LooperTools>,
        steps: &mut Vec<TurnStep>,
    ) -> Result<()> {
        let mut builder = self
            .client
            .generate_content()
            .with_system_prompt(&self.system_message)
            .with_messages(self.messages.clone())
            .with_thinking_budget(-1)
            .with_thoughts_included(true);

        if let Some(tool) = &self.tool {
            builder = builder.with_tool(tool.clone());
        }

        let response = builder.execute().await?;

        let mut thinking = Vec::new();
        let mut text = None;
        let mut func_calls: Vec<(gemini_rust::FunctionCall, Option<String>)> = Vec::new();
        let mut assistant_parts: Vec<Part> = Vec::new();

        for candidate in &response.candidates {
            if let Some(parts) = &candidate.content.parts {
                for part in parts {
                    match part {
                        Part::Text {
                            text: t,
                            thought,
                            thought_signature: _,
                        } => {
                            if *thought == Some(true) {
                                thinking.push(ThinkingBlock { content: t.clone() });
                            } else {
                                text = Some(t.clone());
                            }
                            assistant_parts.push(part.clone());
                        }
                        Part::FunctionCall {
                            function_call,
                            thought_signature,
                        } => {
                            func_calls.push((function_call.clone(), thought_signature.clone()));
                            assistant_parts.push(part.clone());
                        }
                        _ => {
                            assistant_parts.push(part.clone());
                        }
                    }
                }
            }
        }

        // Push assistant message to history
        if !assistant_parts.is_empty() {
            self.messages.push(Message {
                content: Content {
                    parts: Some(assistant_parts),
                    role: Some(Role::Model),
                },
                role: Role::Model,
            });
        }

        // Execute tool calls if any
        let mut tool_call_records = Vec::new();

        if !func_calls.is_empty() {
            let tr = tools_runner.clone();
            let mut tool_join_set = JoinSet::new();

            for (fc, _thought_sig) in func_calls {
                let tr = tr.clone();
                let tool_id = uuid::Uuid::new_v4().to_string();
                tool_join_set.spawn(async move {
                    let result = tr.run_tool(fc.name.clone(), fc.args.clone()).await;
                    (result, fc, tool_id)
                });
            }

            let mut function_response_parts: Vec<Part> = Vec::new();

            while let Some(result) = tool_join_set.join_next().await {
                match result {
                    Ok((result, fc, tool_id)) => {
                        tool_call_records.push(ToolCallRecord {
                            id: tool_id,
                            name: fc.name.clone(),
                            args: fc.args.clone(),
                            result: result.clone(),
                        });

                        function_response_parts.push(Part::FunctionResponse {
                            function_response: FunctionResponse {
                                name: fc.name.clone(),
                                response: Some(result),
                            },
                        });
                    }
                    Err(e) => {
                        eprintln!(
                            "Join Error occured when collecting tool call results | Error: {}",
                            e
                        );
                    }
                }
            }

            // Push function response message to history
            self.messages.push(Message {
                content: Content {
                    parts: Some(function_response_parts),
                    role: Some(Role::User),
                },
                role: Role::User,
            });

            steps.push(TurnStep {
                thinking,
                text,
                tool_calls: tool_call_records,
            });

            // Recurse to handle follow-up
            return self.inner_send_message(tr, steps).await;
        }

        steps.push(TurnStep {
            thinking,
            text,
            tool_calls: tool_call_records,
        });

        Ok(())
    }
}

#[async_trait]
impl ChatHandler for GeminiNonStreamingHandler {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str,
        tools_runner: Arc<dyn LooperTools>,
    ) -> Result<TurnResult> {
        if let Some(MessageHistory::Messages(m)) = message_history {
            let messages: Vec<Message> = serde_json::from_value(m)?;
            self.messages = messages;
        }

        self.messages.push(Message::user(message));

        let mut steps = Vec::new();
        self.inner_send_message(tools_runner, &mut steps).await?;

        let final_text = steps.iter().rev().find_map(|s| s.text.clone());

        let message_history = MessageHistory::Messages(serde_json::to_value(&self.messages)?);

        Ok(TurnResult {
            steps,
            final_text,
            message_history,
        })
    }

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>) {
        if tools.is_empty() {
            self.tool = None;
        } else {
            self.tool = Some(to_gemini_tool(tools));
        }
    }
}
