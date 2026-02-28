use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{chat::ReasoningEffort, responses::{
        CreateResponseArgs, FunctionCallOutput, FunctionCallOutputItemParam, FunctionToolCall, InputItem, InputParam, Item, OutputItem, Reasoning, ReasoningSummary, ResponseStreamEvent, Tool
    }},
};

use async_recursion::async_recursion;
use async_trait::async_trait;

use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::{mpsc::Sender, oneshot};

use crate::{
    looper::AgentLoopState, services::ChatHandler, types::{HandlerToLooperMessage, HandlerToLooperToolCallRequest, LooperToolDefinition}
};

pub struct OpenAIResponsesHandler {
    client: Client<OpenAIConfig>,
    previous_response_id: Option<String>,
    sender: Sender<HandlerToLooperMessage>,
    tools: Vec<Tool>,
    instructions: String,
    loop_state: AgentLoopState
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
            loop_state: AgentLoopState::Continue("".to_string())
        })
    }

    #[async_recursion]
    async fn inner_send_message(&mut self, input: Option<InputParam>) -> Result<String> {
        let mut builder = CreateResponseArgs::default();
        match input {
            Some(i) => {
                builder
                    .model("gpt-5.2")
                    .input(i)
                    .tools(self.tools.clone())
                    .reasoning(Reasoning {
                          effort: Some(ReasoningEffort::High),
                          summary: Some(ReasoningSummary::Concise),
                      })
                    .instructions(self.instructions.clone());

            },
            None => {
                builder
                    .model("gpt-5.2")
                    .tools(self.tools.clone())
                    .reasoning(Reasoning {
                          effort: Some(ReasoningEffort::High),
                          summary: Some(ReasoningSummary::Concise),
                      })
                    .instructions(self.instructions.clone());
            }
        }

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
                    let text = delta.delta.clone();
                    self.sender
                        .send(HandlerToLooperMessage::Thinking(text))
                        .await
                        .unwrap();
                }
                Ok(ResponseStreamEvent::ResponseReasoningSummaryTextDone(_)) => {
                    self.sender
                        .send(HandlerToLooperMessage::ThinkingComplete)
                        .await
                        .unwrap();
                }
                Ok(ResponseStreamEvent::ResponseOutputItemDone(item_done)) => {
                    if let OutputItem::FunctionCall(fc) = item_done.item {
                        let (tx, rx) = oneshot::channel();
                        let args = serde_json::from_str(&fc.arguments)?;

                        self.handle_agent_loop_state(&fc.name, &args);

                        let tcr = HandlerToLooperToolCallRequest {
                            id: fc.call_id.clone(),
                            name: fc.name.clone(),
                            args,
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

            return self.inner_send_message(Some(InputParam::Items(input_items))).await;
        }

        Ok(assistant_res_buf.join(""))
    }

    fn handle_agent_loop_state(&mut self, name: &str, args: &Value) {
        if name != "set_agent_loop_state" { return; }

        match args.get("state") {
            Some(s) => {
                if s == "done" {
                    self.loop_state = AgentLoopState::Done; 
                }
            },
            None => {
                // If the model responds with a state that isn't supported
                // we should just end to avoid infinite loop
                self.loop_state = AgentLoopState::Done; 
            }
        }

    }

}

#[async_trait]
impl ChatHandler for OpenAIResponsesHandler {
    async fn send_message(&mut self, message: &str) -> Result<()> {
        // reset loop state to continue on each message send
        self.loop_state = AgentLoopState::Continue("".to_string());

        let input = InputParam::Text(message.to_string());
        self.inner_send_message(Some(input)).await?;

        while let AgentLoopState::Continue(_) = &self.loop_state {
            self.inner_send_message(None).await?;
        }

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

    fn set_continue(&mut self) {
        self.loop_state = AgentLoopState::Continue("".to_string());
    }
}
