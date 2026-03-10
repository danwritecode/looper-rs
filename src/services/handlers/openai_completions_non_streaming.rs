use std::sync::Arc;

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessage,
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestToolMessage, ChatCompletionRequestUserMessageArgs,
        ChatCompletionTools, CreateChatCompletionRequestArgs, FinishReason,
        ReasoningEffort,
    },
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::{
    services::ChatHandler,
    tools::LooperTools,
    types::{
        LooperToolDefinition, MessageHistory,
        turn::{ToolCallRecord, TurnResult, TurnStep},
    },
};

pub struct OpenAINonStreamingChatHandler {
    client: Client<OpenAIConfig>,
    model: String,
    messages: Vec<ChatCompletionRequestMessage>,
    tools: Vec<ChatCompletionTools>,
}

impl OpenAINonStreamingChatHandler {
    pub fn new(model: &str, system_message: &str) -> Result<Self> {
        let client = Client::new();
        let system_message = ChatCompletionRequestSystemMessageArgs::default()
            .content(system_message)
            .build()?
            .into();

        Ok(OpenAINonStreamingChatHandler {
            client,
            model: model.to_string(),
            messages: vec![system_message],
            tools: Vec::new(),
        })
    }

    #[async_recursion]
    async fn inner_send_message(
        &mut self,
        tools_runner: Option<&'async_recursion Arc<Mutex<dyn LooperTools>>>,
        steps: &mut Vec<TurnStep>,
    ) -> Result<()> {
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_completion_tokens(50000u32)
            .messages(self.messages.clone())
            .tools(self.tools.clone())
            .reasoning_effort(ReasoningEffort::Low)
            .build()?;

        let response = self.client.chat().create(request).await?;

        let choice = match response.choices.into_iter().next() {
            Some(c) => c,
            None => {
                steps.push(TurnStep {
                    thinking: Vec::new(),
                    text: None,
                    tool_calls: Vec::new(),
                });
                return Ok(());
            }
        };

        let message = choice.message;
        let text = message.content.clone();
        let has_tool_calls = matches!(choice.finish_reason, Some(FinishReason::ToolCalls));

        if has_tool_calls {
            let tool_calls_list = message.tool_calls.clone().unwrap_or_default();

            // Push assistant message with tool calls to history
            self.messages.push(
                ChatCompletionRequestAssistantMessage {
                    content: message.content.clone().map(|c| c.into()),
                    tool_calls: Some(tool_calls_list.clone()),
                    ..Default::default()
                }
                .into(),
            );

            // Execute tool calls
            let mut tool_call_records = Vec::new();

            for tc in &tool_calls_list {
                let ChatCompletionMessageToolCalls::Function(func_call) = tc else {
                    continue;
                };

                let args: Value = serde_json::from_str(&func_call.function.arguments)?;

                let result = match tools_runner {
                    Some(runner) => {
                        let runner = runner.lock().await;
                        runner.run_tool(&func_call.function.name, args.clone()).await
                    },
                    None => serde_json::json!({"error": "No tools runner available"}),
                };

                tool_call_records.push(ToolCallRecord {
                    id: func_call.id.clone(),
                    name: func_call.function.name.clone(),
                    args,
                    result: result.clone(),
                });

                // Push tool result message to history
                self.messages.push(
                    ChatCompletionRequestToolMessage {
                        content: result.to_string().into(),
                        tool_call_id: func_call.id.clone(),
                    }
                    .into(),
                );
            }

            steps.push(TurnStep {
                thinking: Vec::new(),
                text,
                tool_calls: tool_call_records,
            });

            // Recurse to handle follow-up
            return self.inner_send_message(tools_runner, steps).await;
        }

        // No tool calls — final response
        // Push assistant message to history
        if let Some(ref content) = text {
            let assistant_msg = ChatCompletionRequestAssistantMessage {
                content: Some(content.clone().into()),
                ..Default::default()
            };
            self.messages.push(assistant_msg.into());
        }

        steps.push(TurnStep {
            thinking: Vec::new(),
            text,
            tool_calls: Vec::new(),
        });

        Ok(())
    }
}

#[async_trait]
impl ChatHandler for OpenAINonStreamingChatHandler {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str,
        tools_runner: Option<&Arc<Mutex<dyn LooperTools>>>,
    ) -> Result<TurnResult> {
        if let Some(MessageHistory::Messages(m)) = message_history {
            let messages: Vec<ChatCompletionRequestMessage> = serde_json::from_value(m)?;
            self.messages = messages;
        }

        let user_message = ChatCompletionRequestUserMessageArgs::default()
            .content(message)
            .build()?
            .into();

        self.messages.push(user_message);

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
            .map(|t| ChatCompletionTools::Function(t.into()))
            .collect();
    }
}
