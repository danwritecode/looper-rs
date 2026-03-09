use serde_json::Value;

pub type Model<'a> = &'a str;

pub enum Handlers<'a> {
    OpenAICompletions(Model<'a>),
    OpenAIResponses(Model<'a>),
    Anthropic(Model<'a>),
}

#[derive(Debug, Clone)]
pub enum MessageHistory {
    /// Serialized Vec<Message> for Anthropic and OpenAI Completions
    Messages(Value),
    /// Server-held conversation state for OpenAI Responses API
    ResponseId(String),
}
