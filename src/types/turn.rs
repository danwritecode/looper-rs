use super::MessageHistory;
use serde_json::Value;

pub struct ThinkingBlock {
    pub content: String,
}

pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub result: Value,
}

pub struct TurnStep {
    pub thinking: Vec<ThinkingBlock>,
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCallRecord>,
}

pub struct TurnResult {
    pub steps: Vec<TurnStep>,
    pub final_text: Option<String>,
    pub message_history: MessageHistory,
}
