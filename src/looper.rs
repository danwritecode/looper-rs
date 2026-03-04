use std::sync::Arc;

use crate::{
    services::{openai_completions::OpenAIChatHandler, openai_responses::OpenAIResponsesHandler, ChatHandler},
    tools::LooperTools,
    types::{HandlerToLooperMessage, Handlers, LooperToHandlerToolCallResult, LooperToInterfaceMessage},
};
use anyhow::Result;
use tokio::sync::{
    Mutex,
    mpsc::{self, Receiver, Sender},
};

pub struct Looper {
    handler: Box<dyn ChatHandler>,
    looper_interface_sender: Sender<LooperToInterfaceMessage>,
    handler_looper_receiver: Arc<Mutex<Receiver<HandlerToLooperMessage>>>,
    tools: Arc<LooperTools>,
}

pub enum AgentLoopState {
    Continue(String),
    Done
}

impl Looper {
    pub fn new(
        handler: Handlers,
        tools: LooperTools,
        looper_interface_sender: Sender<LooperToInterfaceMessage>
    ) -> Result<Self> {

        // TODO: Set this to something reasonable, totally just guessed at 10k 
        let (handler_looper_sender, handler_looper_receiver) = mpsc::channel(10000); // for handler to send messages to looper
        let handler_looper_receiver = Arc::new(Mutex::new(handler_looper_receiver));
        let system_message = get_system_message();

        let mut handler: Box<dyn ChatHandler> = match handler {
            Handlers::OpenAIResponses => {
                Box::new(OpenAIResponsesHandler::new(
                    handler_looper_sender,
                    &system_message,
                )?)
            },
            Handlers::OpenAICompletions => {
                Box::new(OpenAIChatHandler::new(
                    handler_looper_sender,
                    &system_message,
                )?)
            }
        };

        handler.set_tools(tools.get_tools());
        let tools = Arc::new(tools);

        Ok(Looper {
            handler,
            looper_interface_sender,
            handler_looper_receiver,
            tools,
        })
    }

    pub async fn send(&mut self, message: &str) -> Result<()> {
        let l_i_s = self.looper_interface_sender.clone();
        let h_l_r = self.handler_looper_receiver.clone();
        let tools = self.tools.clone();

        tokio::spawn(async move {
            let mut h_l_r = h_l_r.lock().await;
            while let Some(message) = h_l_r.recv().await {
                match message {
                    HandlerToLooperMessage::Assistant(m) => {
                        l_i_s
                            .send(LooperToInterfaceMessage::Assistant(m))
                            .await
                            .unwrap();
                    }
                    HandlerToLooperMessage::Thinking(m) => {
                        l_i_s
                            .send(LooperToInterfaceMessage::Thinking(m))
                            .await
                            .unwrap();
                    }
                    HandlerToLooperMessage::ThinkingComplete => {
                        l_i_s
                            .send(LooperToInterfaceMessage::ThinkingComplete)
                            .await
                            .unwrap();
                    }
                    HandlerToLooperMessage::ToolCallRequest(tc) => {
                        l_i_s
                            .send(LooperToInterfaceMessage::ToolCall(tc.name.clone()))
                            .await
                            .unwrap();

                        let response = tools.run_tool(&tc.name, tc.args).await;

                        let tc_result = LooperToHandlerToolCallResult {
                            id: tc.id,
                            value: response,
                        };

                        tc.tool_result_channel.send(tc_result).unwrap();
                    }
                    HandlerToLooperMessage::TurnComplete => {
                        l_i_s
                            .send(LooperToInterfaceMessage::TurnComplete)
                            .await
                            .unwrap();
                    }
                }
            }
        });

        self.handler.send_message(message).await?;

        Ok(())
    }
}

// fn get_system_message() -> String {
//     format!("You think deeply about everything before replying.")
// }

fn get_system_message() -> String {
    include_str!("../prompts/system_prompt.txt").to_string()
}
