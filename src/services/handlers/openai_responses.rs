use std::sync::Arc;

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        chat::ReasoningEffort,
        responses::{
            CreateResponseArgs, FunctionCallOutput, FunctionCallOutputItemParam, FunctionToolCall,
            InputItem, InputParam, Item, OutputItem, Reasoning, ReasoningSummary,
            ResponseStreamEvent, Tool,
        },
    },
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;
use tokio::task::JoinSet;

use crate::{
    services::StreamingChatHandler,
    tools::LooperTools,
    types::{
        HandlerToLooperMessage, HandlerToLooperToolCallRequest, LooperToolDefinition,
        MessageHistory,
    },
};

pub struct OpenAIResponsesHandler {
    client: Client<OpenAIConfig>,
    model: String,
    previous_response_id: Option<String>,
    sender: tokio::sync::mpsc::Sender<HandlerToLooperMessage>,
    tools: Vec<Tool>,
    instructions: String,
}

impl OpenAIResponsesHandler {
    pub fn new(
        sender: tokio::sync::mpsc::Sender<HandlerToLooperMessage>,
        model: &str,
        system_message: &str,
    ) -> Result<Self> {
        let client = Client::new();

        Ok(OpenAIResponsesHandler {
            client,
            model: model.to_string(),
            previous_response_id: None,
            sender,
            tools: Vec::new(),
            instructions: system_message.to_string(),
        })
    }

    #[async_recursion]
    async fn inner_send_message(
        &mut self,
        input: Option<InputParam>,
        tools_runner: Arc<dyn LooperTools>,
    ) -> Result<String> {
        let mut builder = CreateResponseArgs::default();
        builder
            .model(&self.model)
            .tools(self.tools.clone())
            .reasoning(Reasoning {
                effort: Some(ReasoningEffort::High),
                summary: Some(ReasoningSummary::Concise),
            })
            .instructions(self.instructions.clone());

        if let Some(i) = input {
            builder.input(i);
        }

        if let Some(ref prev_id) = self.previous_response_id {
            builder.previous_response_id(prev_id);
        }

        let request = builder.build()?;
        let mut stream = self.client.responses().create_stream(request).await?;

        let mut assistant_res_buf = Vec::new();
        let mut function_calls: Vec<FunctionToolCall> = Vec::new();
        let mut tool_join_set = JoinSet::new();
        let mut response_id: Option<String> = None;

        while let Some(event) = stream.next().await {
            match event {
                Ok(ResponseStreamEvent::ResponseOutputTextDelta(delta)) => {
                    let text = delta.delta.clone();
                    assistant_res_buf.push(text.clone());
                    self.sender
                        .send(HandlerToLooperMessage::Assistant(text))
                        .await?;
                }
                Ok(ResponseStreamEvent::ResponseReasoningSummaryTextDelta(delta)) => {
                    let text = delta.delta.clone();
                    self.sender
                        .send(HandlerToLooperMessage::Thinking(text))
                        .await?;
                }
                Ok(ResponseStreamEvent::ResponseReasoningSummaryTextDone(_)) => {
                    self.sender
                        .send(HandlerToLooperMessage::ThinkingComplete)
                        .await?;
                }
                Ok(ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(delta)) => {
                    self.sender
                        .send(HandlerToLooperMessage::ToolCallPending(
                            delta.item_id.clone(),
                        ))
                        .await?;
                }
                Ok(ResponseStreamEvent::ResponseOutputItemDone(item_done)) => {
                    if let OutputItem::FunctionCall(fc) = item_done.item {
                        let tcr = HandlerToLooperToolCallRequest {
                            id: fc.call_id.clone(),
                            name: fc.name.clone(),
                            args: serde_json::from_str(&fc.arguments).unwrap_or_default(),
                        };

                        self.sender
                            .send(HandlerToLooperMessage::ToolCallRequest(tcr.clone()))
                            .await?;

                        let tr = tools_runner.clone();
                        let fc_clone = fc.clone();
                        tool_join_set.spawn(async move {
                            let args: Value =
                                serde_json::from_str(&fc_clone.arguments).unwrap_or_default();
                            let result = tr.run_tool(fc_clone.name.clone(), args).await;
                            (fc_clone.call_id.clone(), result)
                        });

                        function_calls.push(fc);
                    }
                }
                Ok(ResponseStreamEvent::ResponseCompleted(completed)) => {
                    response_id = Some(completed.response.id.clone());
                }
                Ok(_) => {}
                Err(err) => {
                    println!("error: {err:?}");
                }
            }
        }

        // Update previous_response_id for conversation continuity
        if let Some(id) = response_id {
            self.previous_response_id = Some(id);
        }

        if !tool_join_set.is_empty() {
            let mut input_items: Vec<InputItem> = Vec::new();

            while let Some(result) = tool_join_set.join_next().await {
                match result {
                    Ok((call_id, value)) => {
                        self.sender
                            .send(HandlerToLooperMessage::ToolCallComplete(call_id.clone()))
                            .await?;

                        input_items.push(InputItem::Item(Item::FunctionCallOutput(
                            FunctionCallOutputItemParam {
                                call_id,
                                output: FunctionCallOutput::Text(value.to_string()),
                                id: None,
                                status: None,
                            },
                        )));
                    }
                    Err(e) => {
                        eprintln!(
                            "Join Error occured when collecting tool call results | Error: {}",
                            e
                        );
                    }
                }
            }

            return self
                .inner_send_message(Some(InputParam::Items(input_items)), tools_runner)
                .await;
        }

        Ok(assistant_res_buf.join(""))
    }
}

#[async_trait]
impl StreamingChatHandler for OpenAIResponsesHandler {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str,
        tools_runner: Arc<dyn LooperTools>,
    ) -> Result<MessageHistory> {
        if let Some(MessageHistory::ResponseId(id)) = message_history {
            self.previous_response_id = Some(id);
        }

        let input = InputParam::Text(message.to_string());
        self.inner_send_message(Some(input), tools_runner).await?;

        self.sender
            .send(HandlerToLooperMessage::TurnComplete)
            .await?;

        Ok(MessageHistory::ResponseId(
            self.previous_response_id.clone().unwrap_or_default(),
        ))
    }

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>) {
        self.tools = tools
            .into_iter()
            .map(|t| Tool::Function(t.into()))
            .collect();
    }
}
