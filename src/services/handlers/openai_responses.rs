use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{chat::ReasoningEffort, responses::{
        CreateResponseArgs, FunctionCallOutput, FunctionCallOutputItemParam,
        FunctionToolCall, InputItem, InputParam, Item, OutputItem,
        ResponseStreamEvent, Tool,
    }},
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use futures::StreamExt;
use tokio::sync::{mpsc::Sender, oneshot};

use crate::{
    services::ChatHandler,
    types::{HandlerToLooperMessage, HandlerToLooperToolCallRequest, LooperToolDefinition},
};

pub struct OpenAIResponsesHandler {
    client: Client<OpenAIConfig>,
    previous_response_id: Option<String>,
    sender: Sender<HandlerToLooperMessage>,
    tools: Vec<Tool>,
    instructions: String,
}

impl OpenAIResponsesHandler {
    pub fn new(sender: Sender<HandlerToLooperMessage>, system_message: &str) -> Result<Self> {
        let client = Client::new();

        Ok(OpenAIResponsesHandler {
            client,
            previous_response_id: None,
            sender,
            tools: Vec::new(),
            instructions: system_message.to_string(),
        })
    }

    #[async_recursion]
    async fn inner_send_message(&mut self, input: InputParam) -> Result<String> {
        let mut builder = CreateResponseArgs::default();
        builder
            .model("gpt-5.2")
            .input(input)
            .tools(self.tools.clone())
            .reasoning(ReasoningEffort::High)
            .instructions(self.instructions.clone());

        if let Some(ref prev_id) = self.previous_response_id {
            builder.previous_response_id(prev_id);
        }

        let request = builder.build()?;
        let mut stream = self.client.responses().create_stream(request).await?;

        let mut assistant_res_buf = Vec::new();
        let mut function_calls: Vec<FunctionToolCall> = Vec::new();
        let mut tool_call_receivers = Vec::new();
        let mut response_id: Option<String> = None;

        while let Some(event) = stream.next().await {
            match event {
                Ok(ResponseStreamEvent::ResponseOutputTextDelta(delta)) => {
                    let text = delta.delta.clone();
                    assistant_res_buf.push(text.clone());
                    self.sender
                        .send(HandlerToLooperMessage::Assistant(text))
                        .await
                        .unwrap();
                }
                Ok(ResponseStreamEvent::ResponseReasoningSummaryTextDelta(delta)) => {
                    println!("Thinking token: {:?}", delta.delta);
                    let text = delta.delta.clone();
                    self.sender
                        .send(HandlerToLooperMessage::Thinking(text))
                        .await
                        .unwrap();
                }
                Ok(ResponseStreamEvent::ResponseOutputItemDone(item_done)) => {
                    if let OutputItem::FunctionCall(fc) = item_done.item {
                        let (tx, rx) = oneshot::channel();

                        let tcr = HandlerToLooperToolCallRequest {
                            id: fc.call_id.clone(),
                            name: fc.name.clone(),
                            args: serde_json::from_str(&fc.arguments)?,
                            tool_result_channel: tx,
                        };

                        self.sender
                            .send(HandlerToLooperMessage::ToolCallRequest(tcr))
                            .await
                            .unwrap();

                        tool_call_receivers.push(rx);
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

        if !function_calls.is_empty() {
            let results = futures::future::join_all(
                tool_call_receivers
                    .into_iter()
                    .map(|rx| async move {
                        let res = rx.await.unwrap();
                        (res.id, res.value)
                    }),
            )
            .await;

            // Pass function call outputs back — the server reconstructs
            // the full context from previous_response_id
            let input_items: Vec<InputItem> = results
                .into_iter()
                .map(|(call_id, value)| {
                    InputItem::Item(Item::FunctionCallOutput(FunctionCallOutputItemParam {
                        call_id,
                        output: FunctionCallOutput::Text(value.to_string()),
                        id: None,
                        status: None,
                    }))
                })
                .collect();

            return self.inner_send_message(InputParam::Items(input_items)).await;
        }

        Ok(assistant_res_buf.join(""))
    }
}

#[async_trait]
impl ChatHandler for OpenAIResponsesHandler {
    async fn send_message(&mut self, message: &str) -> Result<()> {
        let input = InputParam::Text(message.to_string());
        self.inner_send_message(input).await?;

        self.sender
            .send(HandlerToLooperMessage::TurnComplete)
            .await?;

        Ok(())
    }

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>) {
        self.tools = tools
            .into_iter()
            .map(|t| Tool::Function(t.into()))
            .collect();
    }
}
