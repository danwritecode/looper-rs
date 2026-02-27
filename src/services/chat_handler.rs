use std::sync::Arc;

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageArgs,
        ChatCompletionRequestDeveloperMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessage,
        ChatCompletionRequestUserMessageArgs, ChatCompletionTool, ChatCompletionTools,
        CreateChatCompletionRequestArgs, FinishReason, FunctionObjectArgs, ReasoningEffort,
    },
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use futures::{StreamExt, future::join_all};
use serde_json::{Value, json};
use tokio::sync::{
    Mutex, mpsc::{Receiver, Sender}, oneshot
};

use crate::types::{HandlerToLooperMessage, HandlerToLooperToolCallRequest, LooperToHandlerMessage};

#[async_trait]
pub trait ChatHandler {
    fn add_message(&mut self, message: &str) -> Result<()>;
    async fn send_message(&mut self) -> Result<()>;
}

pub struct OpenAIChatHandler {
    client: Client<OpenAIConfig>,
    messages: Vec<ChatCompletionRequestMessage>,
    sender: Sender<HandlerToLooperMessage>,
    receiver: Arc<Mutex<Receiver<LooperToHandlerMessage>>>,
}

impl OpenAIChatHandler {
    pub fn new(
        sender: Sender<HandlerToLooperMessage>,
        receiver: Receiver<LooperToHandlerMessage>,
        system_message: &str,
    ) -> Result<Self> {
        let client = Client::new();
        let system_message = ChatCompletionRequestSystemMessageArgs::default()
            .content(system_message)
            .build()?
            .into();

        let messages = vec![system_message];
        let receiver = Arc::new(Mutex::new(receiver));

        Ok(OpenAIChatHandler {
            client,
            messages,
            sender,
            receiver,
        })
    }

    pub fn add_tool_response(&mut self, id: String, response: Value) {
        let message = ChatCompletionMessageToolCall {
            id,
            function: Default::default()
        };

        // self.messages.push(message);

        // self.messages.push(message);

    }

    #[async_recursion]
    async fn inner_send_message(&mut self) -> Result<String> {
        let request = CreateChatCompletionRequestArgs::default()
            .model("gpt-5.2")
            .max_completion_tokens(50000u32)
            .messages(self.messages.clone())
            .tools(build_tools()?)
            .reasoning_effort(ReasoningEffort::Low)
            .build()?;

        let mut stream = self.client.chat().create_stream(request).await?;
        let mut assistant_res_buf = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_receivers = Vec::new();

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
                            }
                        }

                        // When tool calls are complete, start executing them immediately
                        if matches!(choice.finish_reason, Some(FinishReason::ToolCalls)) {
                            // Spawn execution tasks for all collected tool calls
                            for tool_call in tool_calls.iter() {
                                let id = tool_call.id.clone();
                                let name = tool_call.function.name.clone();
                                let args = serde_json::from_str(&tool_call.function.arguments.clone())?;
                                let (tx, rx) = oneshot::channel();

                                let tcr = HandlerToLooperToolCallRequest {
                                    id,
                                    name,
                                    args,
                                    tool_result_channel: tx
                                };

                                self.sender
                                    .send(HandlerToLooperMessage::ToolCallRequest(tcr))
                                    .await
                                    .unwrap();

                                tool_call_receivers.push(rx);
                            }
                        }
                    }
                }
                Err(err) => {
                    println!("error: {err:?}");
                }
            }
        }

        let results = futures::future::join_all(
            tool_call_receivers.into_iter().map(|rx| async move {
                let res = rx.await.unwrap();
                (res.id, res.value)
            })
        ).await;

        // Wait for all tool call executions to complete (outside the stream loop)
        if !results.is_empty() {
            let mut tool_responses = Vec::new();

            for r in results {
                let (tool_call_id, response) = r;
                tool_responses.push((tool_call_id, response));
            }

            // Add assistant message with tool calls
            let assistant_tool_calls: Vec<ChatCompletionMessageToolCalls> = tool_calls
                .iter()
                .map(|tc| tc.clone().into())
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

            return self.inner_send_message().await;
        }

        Ok(assistant_res_buf.join(""))
    }

    pub async fn send_message(&mut self, message: &str) -> Result<()> {
        let message = ChatCompletionRequestUserMessageArgs::default()
            .content(message)
            .build()?
            .into();

        self.messages.push(message);

        let response = self.inner_send_message().await?;

        let message = ChatCompletionRequestAssistantMessageArgs::default()
            .content(response.clone())
            .build()?
            .into();

        self.messages.push(message);


        let end_res = HandlerToLooperMessage::TurnComplete;
        self.sender.send(end_res).await?;

        Ok(())
    }
}

fn build_tools() -> Result<Vec<ChatCompletionTools>> {
    Ok(vec![
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("read_file")
                    .description("Read the contents of a file at a given path. Returns the file contents as a string.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "The file path to read (absolute or relative to cwd)" }
                        },
                        "required": ["path"]
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("write_file")
                    .description("Write content to a file. Creates the file if it doesn't exist, overwrites if it does.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "The file path to write to" },
                            "content": { "type": "string", "description": "The content to write to the file" }
                        },
                        "required": ["path", "content"]
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("list_directory")
                    .description("List files and directories at the given path. Returns names with '/' suffix for directories.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "The directory path to list (default: current directory)" }
                        },
                        "required": []
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("grep")
                    .description("Search for a regex pattern in files. Recursively searches the given path and returns matching lines with file paths and line numbers.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string", "description": "The regex pattern to search for" },
                            "path": { "type": "string", "description": "The file or directory to search in (default: current directory)" }
                        },
                        "required": ["pattern"]
                    }))
                    .build()?,
            }
        ),
        ChatCompletionTools::Function(
            ChatCompletionTool {
                function: FunctionObjectArgs::default()
                    .name("find_files")
                    .description("Find files matching a glob pattern recursively. Returns a list of matching file paths.")
                    .parameters(json!({
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string", "description": "Glob pattern to match, e.g. '**/*.rs', 'src/**/*.toml'" },
                            "path": { "type": "string", "description": "The root directory to search from (default: current directory)" }
                        },
                        "required": ["pattern"]
                    }))
                    .build()?,
            }
        ),
    ])
}
