use std::sync::Arc;

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessage,
        ChatCompletionRequestUserMessageArgs, ChatCompletionTools, CreateChatCompletionRequestArgs,
        FinishReason, ReasoningEffort,
    },
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use futures::StreamExt;
use tokio::task::JoinSet;

use serde_json::Value;

use crate::{
    services::StreamingChatHandler,
    tools::LooperTools,
    types::{
        HandlerToLooperMessage, HandlerToLooperToolCallRequest, LooperToolDefinition,
        MessageHistory,
    },
};

pub struct OpenAIChatHandler {
    client: Client<OpenAIConfig>,
    model: String,
    messages: Vec<ChatCompletionRequestMessage>,
    sender: tokio::sync::mpsc::Sender<HandlerToLooperMessage>,
    tools: Vec<ChatCompletionTools>,
}

impl OpenAIChatHandler {
    pub fn new(
        sender: tokio::sync::mpsc::Sender<HandlerToLooperMessage>,
        model: &str,
        system_message: &str,
    ) -> Result<Self> {
        let client = Client::new();
        let system_message = ChatCompletionRequestSystemMessageArgs::default()
            .content(system_message)
            .build()?
            .into();

        let messages = vec![system_message];
        let tools = Vec::new();

        Ok(OpenAIChatHandler {
            client,
            model: model.to_string(),
            messages,
            sender,
            tools,
        })
    }

    #[async_recursion]
    async fn inner_send_message(&mut self, tools_runner: Arc<dyn LooperTools>) -> Result<String> {
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_completion_tokens(50000u32)
            .messages(self.messages.clone())
            .tools(self.tools.clone())
            .reasoning_effort(ReasoningEffort::Low)
            .build()?;

        let mut stream = self.client.chat().create_stream(request).await?;
        let mut assistant_res_buf = Vec::new();
        let mut tool_calls: Vec<ChatCompletionMessageToolCall> = Vec::new();
        let mut tool_join_set = JoinSet::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    for choice in response.choices.into_iter() {
                        // handle text chunk
                        if let Some(content) = choice.delta.content {
                            assistant_res_buf.push(content.clone());
                            self.sender
                                .send(HandlerToLooperMessage::Assistant(content))
                                .await
                                .unwrap();
                        }

                        // handle tool call chunks
                        if let Some(tool_call_chunks) = choice.delta.tool_calls {
                            for chunk in tool_call_chunks {
                                let index = chunk.index as usize;

                                // Ensure we have enough space in the vector
                                while tool_calls.len() <= index {
                                    tool_calls.push(ChatCompletionMessageToolCall {
                                        id: String::new(),
                                        function: Default::default(),
                                    });
                                }

                                // Update the tool call with chunk data
                                let tool_call = &mut tool_calls[index];
                                if let Some(id) = chunk.id {
                                    tool_call.id = id;
                                }
                                if let Some(function_chunk) = chunk.function {
                                    if let Some(name) = function_chunk.name {
                                        tool_call.function.name = name;
                                    }
                                    if let Some(arguments) = function_chunk.arguments {
                                        tool_call.function.arguments.push_str(&arguments);
                                    }
                                }

                                self.sender
                                    .send(HandlerToLooperMessage::ToolCallPending(
                                        tool_calls[index].id.clone(),
                                    ))
                                    .await?;
                            }
                        }

                        // When tool calls are complete, spawn parallel execution
                        if matches!(choice.finish_reason, Some(FinishReason::ToolCalls)) {
                            for tool_call in tool_calls.iter() {
                                let tcr = HandlerToLooperToolCallRequest {
                                    id: tool_call.id.clone(),
                                    name: tool_call.function.name.clone(),
                                    args: serde_json::from_str(&tool_call.function.arguments)
                                        .unwrap_or_default(),
                                };

                                self.sender
                                    .send(HandlerToLooperMessage::ToolCallRequest(tcr.clone()))
                                    .await?;

                                let tr = tools_runner.clone();
                                let tc_id = tool_call.id.clone();
                                let tc_name = tool_call.function.name.clone();
                                let tc_args: Value =
                                    serde_json::from_str(&tool_call.function.arguments)
                                        .unwrap_or_default();

                                tool_join_set.spawn(async move {
                                    let result = tr.run_tool(tc_name, tc_args).await;
                                    (tc_id, result)
                                });
                            }
                        }
                    }
                }
                Err(err) => {
                    println!("error: {err:?}");
                }
            }
        }

        // Wait for all tool call executions to complete
        if !tool_join_set.is_empty() {
            // Add assistant message with tool calls
            let assistant_tool_calls: Vec<ChatCompletionMessageToolCalls> =
                tool_calls.iter().map(|tc| tc.clone().into()).collect();

            self.messages.push(
                ChatCompletionRequestAssistantMessage {
                    content: None,
                    tool_calls: Some(assistant_tool_calls),
                    ..Default::default()
                }
                .into(),
            );

            while let Some(result) = tool_join_set.join_next().await {
                match result {
                    Ok((tool_call_id, response)) => {
                        self.sender
                            .send(HandlerToLooperMessage::ToolCallComplete(
                                tool_call_id.clone(),
                            ))
                            .await?;

                        self.messages.push(
                            ChatCompletionRequestToolMessage {
                                content: response.to_string().into(),
                                tool_call_id,
                            }
                            .into(),
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "Join Error occured when collecting tool call results | Error: {}",
                            e
                        );
                    }
                }
            }

            return self.inner_send_message(tools_runner).await;
        }

        Ok(assistant_res_buf.join(""))
    }
}

#[async_trait]
impl StreamingChatHandler for OpenAIChatHandler {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str,
        tools_runner: Arc<dyn LooperTools>,
    ) -> Result<MessageHistory> {
        if let Some(MessageHistory::Messages(m)) = message_history {
            let messages: Vec<ChatCompletionRequestMessage> = serde_json::from_value(m)?;
            self.messages = messages;
        }

        let message = ChatCompletionRequestUserMessageArgs::default()
            .content(message)
            .build()?
            .into();

        self.messages.push(message);

        self.inner_send_message(tools_runner).await?;

        self.sender
            .send(HandlerToLooperMessage::TurnComplete)
            .await?;

        let messages = serde_json::to_value(&self.messages)?;

        Ok(MessageHistory::Messages(messages))
    }

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>) {
        let tools = tools
            .into_iter()
            .map(|t| ChatCompletionTools::Function(t.into()))
            .collect::<Vec<ChatCompletionTools>>();

        self.tools = tools;
    }
}
