use std::sync::Arc;

use async_anthropic::{
    Client,
    types::{
        CreateMessagesRequestBuilder, Message, MessageContent, MessageContentList,
        MessageRole, Thinking, Tool, ToolResultBuilder,
    },
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::{
    services::ChatHandler,
    tools::LooperTools,
    types::{
        LooperToolDefinition, MessageHistory,
        turn::{ThinkingBlock, ToolCallRecord, TurnResult, TurnStep},
    },
};

pub struct AnthropicNonStreamingHandler {
    client: Client,
    model: String,
    system_message: String,
    messages: Vec<Message>,
    tools: Vec<Tool>,
}

impl AnthropicNonStreamingHandler {
    pub fn new(model: &str, system_message: &str) -> Result<Self> {
        let client = Client::default();

        Ok(AnthropicNonStreamingHandler {
            client,
            model: model.to_string(),
            system_message: system_message.to_string(),
            messages: vec![],
            tools: Vec::new(),
        })
    }

    #[async_recursion]
    async fn inner_send_message(
        &mut self,
        tools_runner: Option<&'async_recursion Arc<Mutex<dyn LooperTools>>>,
        steps: &mut Vec<TurnStep>,
    ) -> Result<()> {
        let request = CreateMessagesRequestBuilder::default()
            .model(&self.model)
            .system(self.system_message.clone())
            .messages(self.messages.clone())
            .tools(self.tools.clone())
            .max_tokens(16384)
            .thinking(Thinking::Adaptive)
            .build()?;

        let response = self.client.messages().create(request).await?;

        let mut thinking = Vec::new();
        let mut text = None;
        let mut tool_uses = Vec::new();
        let mut assistant_content: Vec<MessageContent> = Vec::new();

        if let Some(content) = &response.content {
            for block in content {
                match block {
                    MessageContent::Thinking(t) => {
                        thinking.push(ThinkingBlock {
                            content: t.thinking.clone(),
                        });
                        assistant_content.push(block.clone());
                    }
                    MessageContent::Text(t) => {
                        text = Some(t.text.clone());
                        assistant_content.push(block.clone());
                    }
                    MessageContent::ToolUse(t) => {
                        tool_uses.push(t.clone());
                        assistant_content.push(block.clone());
                    }
                    _ => {
                        assistant_content.push(block.clone());
                    }
                }
            }
        }

        // Push assistant message to history
        if !assistant_content.is_empty() {
            self.messages.push(Message {
                role: MessageRole::Assistant,
                content: MessageContentList(assistant_content),
            });
        }

        // Execute tool calls if any
        let mut tool_call_records = Vec::new();

        if !tool_uses.is_empty() {
            for tool_use in &tool_uses {
                let result = match tools_runner {
                    Some(runner) => {
                        let runner = runner.lock().await;
                        runner.run_tool(&tool_use.name, tool_use.input.clone()).await
                    },
                    None => serde_json::json!({"error": "No tools runner available"}),
                };

                tool_call_records.push(ToolCallRecord {
                    id: tool_use.id.clone(),
                    name: tool_use.name.clone(),
                    args: tool_use.input.clone(),
                    result: result.clone(),
                });

                // Push tool result message to history
                self.messages.push(Message {
                    role: MessageRole::User,
                    content: MessageContentList(vec![
                        MessageContent::ToolResult(
                            ToolResultBuilder::default()
                                .tool_use_id(&tool_use.id)
                                .content(result.to_string())
                                .build()?
                        )
                    ]),
                });
            }

            steps.push(TurnStep {
                thinking,
                text,
                tool_calls: tool_call_records,
            });

            // Recurse to handle follow-up
            return self.inner_send_message(tools_runner, steps).await;
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
impl ChatHandler for AnthropicNonStreamingHandler {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str,
        tools_runner: Option<&Arc<Mutex<dyn LooperTools>>>,
    ) -> Result<TurnResult> {
        if let Some(MessageHistory::Messages(m)) = message_history {
            let messages: Vec<Message> = serde_json::from_value(m)?;
            self.messages = messages;
        }

        self.messages.push(Message {
            role: MessageRole::User,
            content: MessageContentList(vec![MessageContent::from(message)]),
        });

        let mut steps = Vec::new();
        self.inner_send_message(tools_runner, &mut steps).await?;

        let final_text = steps
            .iter()
            .rev()
            .find_map(|s| s.text.clone());

        let message_history = MessageHistory::Messages(serde_json::to_value(&self.messages)?);

        Ok(TurnResult {
            steps,
            final_text,
            message_history,
        })
    }

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>) {
        self.tools = tools
            .into_iter()
            .map(|t| t.into())
            .collect();
    }
}
