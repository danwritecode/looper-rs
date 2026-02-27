use serde_json::Value;

type Name = String;
type Message = String;

#[derive(Debug)]
pub enum HandlerToLooperMessage {
    Assistant(Message),
    ToolCallRequest(HandlerToLooperToolCallRequest)
}

#[derive(Debug)]
pub struct HandlerToLooperToolCallRequest {
    pub id: String,
    pub name: String,
    pub args: Value
}


/// Looper to Handler Messages
#[derive(Debug)]
pub enum LooperToHandlerMessage {
    ToolCallResult(LooperToHandlerToolCallResult)
}

#[derive(Debug)]
pub struct LooperToHandlerToolCallResult {
    pub id: String,
    pub value: Value
}


#[derive(Debug)]
pub enum LooperToInterfaceMessage {
    Assistant(Message),
    ToolCall(Name)
}
