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

use async_trait::async_trait;

use anyhow::Result;
use futures::StreamExt;
use serde_json::json;
use tokio::sync::{
    mpsc::{Receiver, Sender},
};

use crate::types::{HandlerToLooperMessage, LooperToHandlerMessage};

// Issues
// 1. Chat Handlers communicated directly with UI. Chat handler should handle CHAT
// 2. Tools are built directly into chat handler, but tools need to be provider agnostic
// 3. The agent loop is built into the chat handler, the agent loop should handle this
// 4. System message built into chat handler

#[async_trait]
pub trait ChatHandler {
    fn add_message(&mut self, message: &str) -> Result<()>;
    async fn send_message(&mut self) -> Result<()>;
}

pub struct OpenAIChatHandler {
    client: Client<OpenAIConfig>,
    messages: Vec<ChatCompletionRequestMessage>,
    sender: Sender<HandlerToLooperMessage>,
    receiver: Receiver<LooperToHandlerMessage>,
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

        Ok(OpenAIChatHandler {
            client,
            messages,
            sender,
            receiver,
        })
    }

    pub async fn send_message(&mut self, message: &str) -> Result<()> {
        let message = ChatCompletionRequestUserMessageArgs::default()
            .content(message)
            .build()?
            .into();

        self.messages.push(message);

        let request = CreateChatCompletionRequestArgs::default()
            .model("gpt-3.5-turbo")
            .max_tokens(512u32)
            .messages(self.messages.clone())
            .build()?;

        let mut stream = self.client.chat().create_stream(request).await?;

        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    for cc in response.choices.into_iter() {
                        if let Some(content) = cc.delta.content {
                            self.sender
                                .send(HandlerToLooperMessage::Assistant(content))
                                .await
                                .unwrap();
                        }
                    }
                }
                Err(err) => {
                    println!("error: {err:?}");
                }
            }
        }

        self.sender
            .send(HandlerToLooperMessage::Assistant("<END>".to_string()))
            .await
            .unwrap();

        Ok(())
    }
}

fn build_tools() -> Result<Vec<ChatCompletionTools>> {
    Ok(vec![
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
