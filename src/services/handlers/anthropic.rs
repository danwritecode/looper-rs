use std::collections::HashMap;

use async_anthropic::{
    Client,
    types::{
        ContentBlockDelta, CreateMessagesRequestBuilder, Message, 
        MessageContent, MessageContentList, MessageRole, MessagesStreamEvent, 
        Thinking, Tool, ToolResultBuilder
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


use crate::{services::StreamingChatHandler, types::{
    HandlerToLooperMessage, HandlerToLooperToolCallRequest, LooperToolDefinition, MessageHistory,
}};

pub struct AnthropicHandler {
    client: Client,
    model: String,
    system_message: String,
    messages: Vec<Message>,
    sender: Sender<HandlerToLooperMessage>,
    tools: Vec<Tool>,
}

impl AnthropicHandler {
    pub fn new(
        sender: Sender<HandlerToLooperMessage>,
        model: &str,
        system_message: &str,
    ) -> Result<Self> {
        let client = Client::default();

        let messages = vec![];
        let tools = Vec::new();

        Ok(AnthropicHandler {
            client,
            model: model.to_string(),
            system_message: system_message.to_string(),
            messages,
            sender,
            tools
        })
    }

    #[async_recursion]
    async fn inner_send_message(&mut self) -> Result<String> {
        let request = CreateMessagesRequestBuilder::default()
            .model(&self.model)
            .system(self.system_message.clone())
            .messages(self.messages.clone())
            .tools(self.tools.clone())
            .max_tokens(16384)
            .thinking(Thinking::Adaptive)
            .build()?;


        let mut stream = self.client.messages().create_stream(request).await;
        let mut tool_call_receivers = Vec::new();
        let mut content_blocks = HashMap::new();
        let mut tool_input_bufs: HashMap<usize, String> = HashMap::new();
        let mut signatures: HashMap<usize, String> = HashMap::new();

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
                                    ContentBlockDelta::ThinkingDelta { thinking } => {
                                        if let MessageContent::Thinking(t) = cb {
                                            t.thinking += &thinking;

                                            self.sender
                                                .send(HandlerToLooperMessage::Thinking(thinking))
                                                .await?;
                                        }
                                    },
                                    ContentBlockDelta::SignatureDelta { signature } => {
                                        if let MessageContent::Thinking(_) = cb {
                                            signatures
                                                .entry(index)
                                                .or_default()
                                                .push_str(&signature);
                                        }
                                    },
                                    ContentBlockDelta::InputJsonDelta { partial_json } => {
                                        if let MessageContent::ToolUse(_) = cb {
                                            tool_input_bufs
                                                .entry(index)
                                                .or_default()
                                                .push_str(&partial_json);

                                            self.sender
                                                .send(HandlerToLooperMessage::ToolCallPending(index))
                                                .await?;
                                        }
                                    }
                                }
                            }
                        },
                        MessagesStreamEvent::ContentBlockStop { index } => {
                            // Send ThinkingComplete when a thinking block ends
                            if let Some(MessageContent::Thinking(_)) = content_blocks.get(&index) {
                                self.sender
                                    .send(HandlerToLooperMessage::ThinkingComplete)
                                    .await?;
                            }

                            // Parse accumulated tool input JSON if present
                            if let Some(raw_input) = tool_input_bufs.remove(&index) {
                                if let Some(MessageContent::ToolUse(t)) = content_blocks.get_mut(&index) {
                                    if !raw_input.is_empty() {
                                        t.input = serde_json::from_str(&raw_input)?;
                                    }
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

        // Build a single assistant message from all accumulated content blocks
        let mut sorted_indices: Vec<usize> = content_blocks.keys().copied().collect();
        sorted_indices.sort();

        let mut assistant_content: Vec<MessageContent> = Vec::new();
        for index in &sorted_indices {
            if let Some(mut block) = content_blocks.remove(index) {
                // Inject signature into thinking blocks
                if let MessageContent::Thinking(ref mut t) = block {
                    if let Some(sig) = signatures.remove(index) {
                        t.signature = Some(sig);
                    }
                }

                // Collect tool call requests
                if let MessageContent::ToolUse(ref t) = block {
                    let (tx, rx) = oneshot::channel();

                    let tcr = HandlerToLooperToolCallRequest {
                        id: t.id.clone(),
                        name: t.name.clone(),
                        args: t.input.clone(),
                        tool_result_channel: tx,
                    };

                    self.sender
                        .send(HandlerToLooperMessage::ToolCallRequest(tcr))
                        .await?;

                    tool_call_receivers.push(rx);
                }

                assistant_content.push(block);
            }
        }

        if !assistant_content.is_empty() {
            self.messages.push(Message {
                role: MessageRole::Assistant,
                content: MessageContentList(assistant_content),
            });
        }

        // Wait for all tool call executions to complete
        let results =
            futures::future::join_all(tool_call_receivers.into_iter().map(|rx| async move {
                let res = rx.await.unwrap();
                (res.id, res.value)
            }))
            .await;

        if !results.is_empty() {
            for (tool_call_id, response) in results {
                self.messages.push(Message {
                    role: MessageRole::User,
                    content: MessageContentList(vec![
                        MessageContent::ToolResult(
                            ToolResultBuilder::default()
                                .tool_use_id(&tool_call_id)
                                .content(response.to_string())
                                .build()?
                        )
                    ]),
                });
            }

            return self.inner_send_message().await;
        }

        Ok(String::new())
    }
}

#[async_trait]
impl StreamingChatHandler for AnthropicHandler {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str
    ) -> Result<MessageHistory> {
        if let Some(MessageHistory::Messages(m)) = message_history {
            let messages: Vec<Message> = serde_json::from_value(m)?;
            self.messages = messages;
        }

        self.messages.push(Message {
            role: MessageRole::User,
            content: MessageContentList(vec![MessageContent::from(message)]),
        });

        self.inner_send_message().await?;

        self.sender
            .send(HandlerToLooperMessage::TurnComplete)
            .await?;

        let messages = serde_json::to_value(&self.messages)?;

        Ok(MessageHistory::Messages(messages))
    }

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>) {
        self.tools = tools
            .into_iter()
            .map(|t| t.into())
            .collect();
    }
}
