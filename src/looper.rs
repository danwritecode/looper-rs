use std::sync::Arc;

use crate::{
    services::{ChatHandler, anthropic::AnthropicHandler, openai_completions::OpenAIChatHandler, openai_responses::OpenAIResponsesHandler},
    tools::{LooperTool, LooperTools, SetAgentLoopStateTool},
    types::{HandlerToLooperMessage, Handlers, LooperToHandlerToolCallResult, LooperToInterfaceMessage},
};
use anyhow::Result;
use serde_json::json;
use tokio::sync::{
    Mutex,
    mpsc::{self, Receiver, Sender},
};

pub struct Looper {
    handler: Box<dyn ChatHandler>,
    looper_interface_sender: Sender<LooperToInterfaceMessage>,
    handler_looper_receiver: Arc<Mutex<Receiver<HandlerToLooperMessage>>>,
    tools: Option<Arc<dyn LooperTools>>,
}

#[derive(Debug)]
pub enum AgentLoopState {
    Continue(String),
    Done
}

impl Looper {
    pub fn new(
        handler_type: Handlers,
        tools: Option<Arc<dyn LooperTools>>,
        looper_interface_sender: Sender<LooperToInterfaceMessage>
    ) -> Result<Self> {
        // TODO: Set this to something reasonable, totally just guessed at 10k
        let (handler_looper_sender, handler_looper_receiver) = mpsc::channel(10000);
        let handler_looper_receiver = Arc::new(Mutex::new(handler_looper_receiver));

        let handler: Box<dyn ChatHandler> = match handler_type {
            Handlers::OpenAIResponses(m) => {
                let mut handler = OpenAIResponsesHandler::new(
                    handler_looper_sender,
                    &m,
                    &get_openai_system_message()
                )?;

                if let Some(t) = &tools {
                    let mut tool_defs = t.get_tools();
                    let set_agent_loop_state = SetAgentLoopStateTool;
                    tool_defs.push(set_agent_loop_state.tool());
                    handler.set_tools(tool_defs);
                }

                Box::new(handler)
            },
            Handlers::OpenAICompletions(m) => {
                let mut handler = OpenAIChatHandler::new(
                    handler_looper_sender,
                    &m,
                    &get_openai_system_message()
                )?;

                if let Some(t) = &tools {
                    let mut tool_defs = t.get_tools();
                    let set_agent_loop_state = SetAgentLoopStateTool;
                    tool_defs.push(set_agent_loop_state.tool());
                    handler.set_tools(tool_defs);
                }

                Box::new(handler)
            },
            Handlers::Anthropic(m) => {
                let mut handler = AnthropicHandler::new(
                    handler_looper_sender,
                    &m,
                    &get_anthropic_system_message()
                )?;

                if let Some(t) = &tools {
                    handler.set_tools(t.get_tools());
                }


                Box::new(handler)
            }
        };

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

                        let response = if tc.name == "set_agent_loop_state" {
                            SetAgentLoopStateTool.execute(&tc.args).await
                        } else {
                            match &tools {
                                Some(t) => t.run_tool(&tc.name, tc.args).await,
                                None => json!({"Error": "Unsupported tool called"})
                            }
                            
                        };

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

fn get_openai_system_message() -> String {
    include_str!("../prompts/system_prompt_openai.txt").to_string()
}

fn get_anthropic_system_message() -> String {
    include_str!("../prompts/system_prompt_anthropic.txt").to_string()
}

// fn get_system_message() -> String {
//     include_str!("../prompts/system_prompt.txt").to_string()
// }
