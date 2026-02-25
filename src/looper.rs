use std::sync::Arc;

use anyhow::Result;

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessage, ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageArgs, ChatCompletionTool, CreateChatCompletionRequestArgs, FinishReason, FunctionObject, FunctionObjectArgs, ReasoningEffort};
use async_openai::Client;
use futures::StreamExt;
use serde_json::json;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub enum LooperResponse {
    Assistant(String),
    ToolCall
}

pub struct Looper {
    client: Client<OpenAIConfig>,
    messages: Vec<ChatCompletionRequestMessage>,
    sender: Sender<LooperResponse>
}

impl Looper {
    pub fn new(sender: Sender<LooperResponse>) -> Self {
        let client = Client::new();
        Looper { messages: vec![], client, sender }
    }

    pub async fn send(&mut self, message: &str) -> Result<()> {
        let client = &self.client;

        let message = ChatCompletionRequestUserMessageArgs::default()
            .content(message)
            .build()?
            .into();

        self.messages.push(message);

        let request = CreateChatCompletionRequestArgs::default()
            .model("gpt-5-mini")
            .max_completion_tokens(50000u32)
            .messages(self.messages.clone())
            .reasoning_effort(ReasoningEffort::Low)
            .tools(ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("get_current_weather")
                    .description("Get the current weather in a given location")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "The city and state, e.g. San Francisco, CA",
                            },
                            "unit": { "type": "string", "enum": ["celsius", "fahrenheit"] },
                        },
                        "required": ["location"],
                    }))
                    .build()?,
            })
            .build()?;

        let mut stream = client.chat().create_stream(request).await?;
        let mut res_buf = Vec::new();
        let mut tool_calls = Vec::new();
        let mut execution_handles = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    for choice in response.choices.iter() {
                        if let Some(ref content) = choice.delta.content {
                            res_buf.push(content.clone());
                            let res = LooperResponse::Assistant(content.clone());
                            self.sender.send(res).await.unwrap();
                        }

                        // Collect tool call chunks
                        if let Some(tool_call_chunks) = &choice.delta.tool_calls {
                            let res = LooperResponse::ToolCall;
                            self.sender.send(res).await.unwrap();

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
                                if let Some(id) = &chunk.id {
                                    tool_call.id = id.clone();
                                }
                                if let Some(function_chunk) = &chunk.function {
                                    if let Some(name) = &function_chunk.name {
                                        tool_call.function.name = name.to_string();
                                    }
                                    if let Some(arguments) = &function_chunk.arguments {
                                        tool_call.function.arguments.push_str(&arguments);
                                    }
                                }
                            }
                        }

                        // When tool calls are complete, start executing them immediately
                        if matches!(choice.finish_reason, Some(FinishReason::ToolCalls)) {
                            // Spawn execution tasks for all collected tool calls
                            for tool_call in tool_calls.iter() {
                                let name = tool_call.function.name.clone();
                                let args = tool_call.function.arguments.clone();
                                let tool_call_id = tool_call.id.clone();

                                let handle = tokio::spawn(async move {
                                    let result = call_function(&name, &args).await;
                                    (tool_call_id, result)
                                });
                                execution_handles.push(handle);
                            }
                        }
                    }
                }
                Err(err) => {
                    println!("error: {err:?}");
                }
            }
        }

        // Wait for all tool call executions to complete (outside the stream loop)
        if !execution_handles.is_empty() {
            let mut tool_responses = Vec::new();
            for handle in execution_handles {
                let (tool_call_id, response) = handle.await?;
                tool_responses.push((tool_call_id, response));
            }

            // Add assistant message with tool calls
            let assistant_tool_calls: Vec<ChatCompletionMessageToolCalls> = tool_calls
                .iter()
                .map(|tc| tc.clone().into()) // From<ChatCompletionMessageToolCall>
                .collect();

            self.messages.push(
                ChatCompletionRequestAssistantMessage {
                    content: None,
                    tool_calls: Some(assistant_tool_calls),
                    ..Default::default()
                }
                .into(),
            );

            // Add tool response messages
            for (tool_call_id, response) in tool_responses {
                self.messages.push(
                    ChatCompletionRequestToolMessage {
                        content: response.to_string().into(),
                        tool_call_id,
                    }
                    .into(),
                );
            }

            // Second stream: get the final response
            let follow_up_request = CreateChatCompletionRequestArgs::default()
                .max_completion_tokens(512u32)
                .model("gpt-5-mini")
                .messages(self.messages.clone())
                .build()?;

            let mut follow_up_stream = client.chat().create_stream(follow_up_request).await?;

            while let Some(result) = follow_up_stream.next().await {
                let response = result?;
                for choice in response.choices {
                    if let Some(content) = &choice.delta.content {
                        let res = LooperResponse::Assistant(content.clone());
                        self.sender.send(res).await.unwrap();
                    }
                }
            }
        }

        let end_res = LooperResponse::Assistant("<END>".to_string());
        self.sender.send(end_res).await.unwrap();

        let response = res_buf.join("");
        let message = ChatCompletionRequestSystemMessageArgs::default()
            .content(response)
            .build()?
            .into();

        self.messages.push(message);

        Ok(())
    }
}

async fn call_function(name: &str, args: &str) -> serde_json::Value {
    match name {
        "get_current_weather" => get_current_weather(args),
        _ => json!({"error": format!("Unknown function: {}", name)}),
    }
}

fn get_current_weather(args: &str) -> serde_json::Value {
    let args: serde_json::Value = args.parse().unwrap_or(json!({}));
    let location = args["location"]
        .as_str()
        .unwrap_or("unknown location")
        .to_string();
    let unit = args["unit"].as_str().unwrap_or("fahrenheit");

    let temperature: i32 = 50;
    let forecast = "sunny";

    json!({
        "location": location,
        "temperature": temperature.to_string(),
        "unit": unit,
        "forecast": forecast
    })
}
