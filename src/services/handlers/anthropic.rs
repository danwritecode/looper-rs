use std::collections::HashMap;

use async_anthropic::{
    Client, 
    types::{
        ContentBlockDelta, CreateMessagesRequestBuilder, 
        Message, MessageBuilder, MessageContent, MessageRole, 
        MessagesStreamEvent, ToolResultBuilder, ToolUseBuilder
    }
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use futures::StreamExt;
use tokio::sync::{
    mpsc::Sender,
    oneshot,
};

use serde_json::Value;

use crate::{services::ChatHandler, types::{
    HandlerToLooperMessage, HandlerToLooperToolCallRequest, LooperToolDefinition,
}};

pub struct AnthropicHandler {
    client: Client,
    system_message: String,
    messages: Vec<Message>,
    sender: Sender<HandlerToLooperMessage>,
    tools: Vec<serde_json::Map<String, Value>>,
}

impl AnthropicHandler {
    pub fn new(
        sender: Sender<HandlerToLooperMessage>,
        system_message: &str,
    ) -> Result<Self> {
        let client = Client::default();

        let messages = vec![];
        let tools = Vec::new();

        Ok(AnthropicHandler {
            client,
            system_message: system_message.to_string(),
            messages,
            sender,
            tools
        })
    }

    #[async_recursion]
    async fn inner_send_message(&mut self) -> Result<String> {
        let request = CreateMessagesRequestBuilder::default()
            .model("claude-sonnet-4-6")
            .system(self.system_message.clone())
            .messages(self.messages.clone())
            .tools(self.tools.clone())
            .build()?;


        let mut stream = self.client.messages().create_stream(request).await;
        let mut tool_call_receivers = Vec::new();
        let mut content_blocks = HashMap::new();
        let mut tool_input_bufs: HashMap<usize, String> = HashMap::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    match response {
                        MessagesStreamEvent::ContentBlockStart { index, content_block } => {
                            content_blocks.insert(index, content_block);
                        },
                        MessagesStreamEvent::ContentBlockDelta { index, delta } => {
                            if let Some(cb) = content_blocks.get_mut(&index) {
                                match delta {
                                    ContentBlockDelta::TextDelta { text } => {
                                        if let MessageContent::Text(t) = cb {
                                            t.text += &text;

                                            self.sender
                                                .send(HandlerToLooperMessage::Assistant(text))
                                                .await?;
                                        }
                                    },
                                    ContentBlockDelta::InputJsonDelta { partial_json } => {
                                        if let MessageContent::ToolUse(_) = cb {
                                            tool_input_bufs
                                                .entry(index)
                                                .or_default()
                                                .push_str(&partial_json);
                                        }
                                    }
                                }
                            }
                        },
                        MessagesStreamEvent::ContentBlockStop { index } => {
                            // Parse accumulated tool input JSON if present
                            if let Some(raw_input) = tool_input_bufs.remove(&index) {
                                if let Some(MessageContent::ToolUse(t)) = content_blocks.get_mut(&index) {
                                    if !raw_input.is_empty() {
                                        t.input = serde_json::from_str(&raw_input)?;
                                    }
                                }
                            }

                            if let Some(cb) = content_blocks.get(&index) {
                                match cb {
                                    MessageContent::ToolUse(t) => {
                                        let id = t.id.clone();
                                        let name = t.name.clone();
                                        let args = t.input.clone();

                                        let message = MessageBuilder::default()
                                            .role(MessageRole::Assistant)
                                            .content(
                                                ToolUseBuilder::default()
                                                    .id(&id)
                                                    .name(&name)
                                                    .input(args.clone())
                                                    .build()?
                                            )
                                            .build()?;

                                        self.messages.push(message);

                                        let (tx, rx) = oneshot::channel();

                                        let tcr = HandlerToLooperToolCallRequest {
                                            id,
                                            name,
                                            args,
                                            tool_result_channel: tx,
                                        };

                                        self.sender
                                            .send(HandlerToLooperMessage::ToolCallRequest(tcr))
                                            .await?;

                                        tool_call_receivers.push(rx);
                                    },
                                    MessageContent::Text(t) => {
                                        let message = MessageBuilder::default()
                                            .role(MessageRole::Assistant)
                                            .content(t.text.clone())
                                            .build()?;

                                        self.messages.push(message);
                                    }
                                    _ => ()
                                }
                            }
                        },
                        _ => ()
                    }

                }
                Err(err) => {
                    println!("error: {err:?}");
                }
            }
        }


        let results =
            futures::future::join_all(tool_call_receivers.into_iter().map(|rx| async move {
                let res = rx.await.unwrap();
                (res.id, res.value)
            }))
            .await;

        // Wait for all tool call executions to complete (outside the stream loop)
        if !results.is_empty() {
            let mut tool_responses = Vec::new();

            for r in results {
                let (tool_call_id, response) = r;
                tool_responses.push((tool_call_id, response));
            }

            // Add tool response messages
            for (tool_call_id, response) in tool_responses {
                let message = MessageBuilder::default()
                    .role(MessageRole::User)
                    .content(
                        ToolResultBuilder::default()
                            .tool_use_id(&tool_call_id)
                            .content(response.to_string())
                            .build()?
                    )
                    .build()?;

                self.messages.push(message);

            }

            return self.inner_send_message().await;
        }

        Ok(String::new())
    }
}

#[async_trait]
impl ChatHandler for AnthropicHandler {
    async fn send_message(&mut self, message: &str) -> Result<()> {
        let message = MessageBuilder::default()
            .role(MessageRole::User)
            .content(message)
            .build()?;

        self.messages.push(message);

        self.inner_send_message().await?;

        self.sender
            .send(HandlerToLooperMessage::TurnComplete)
            .await?;

        Ok(())
    }

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>) {
        self.tools = tools
            .into_iter()
            .map(|t| t.into())
            .collect();
    }
}
