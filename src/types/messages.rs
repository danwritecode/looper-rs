use serde_json::Value;

type Name = String;
type Message = String;
type ToolId = String;

#[derive(Debug)]
pub enum HandlerToLooperMessage {
    Assistant(Message),
    Thinking(Message),
    ThinkingComplete,
    ToolCallPending(ToolId),
    ToolCallRequest(HandlerToLooperToolCallRequest),
    ToolCallComplete(ToolId),
    TurnComplete,
}

#[derive(Debug, Clone)]
pub struct HandlerToLooperToolCallRequest {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug)]
pub struct LooperToHandlerToolCallResult {
    pub id: String,
    pub value: Value,
}

#[derive(Debug)]
pub enum LooperToInterfaceMessage {
    Assistant(Message),
    Thinking(Message),
    ThinkingComplete,
    ToolCallPending(ToolId),
    ToolCall(Name),
    ToolCallComplete(ToolId),
    TurnComplete,
}
