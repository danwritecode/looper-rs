use crate::types::turn::TurnStep;
use async_openai::types::chat::ChatChoice;

impl From<ChatChoice> for TurnStep {
    fn from(choice: ChatChoice) -> Self {
        let text = choice.message.content;

        TurnStep {
            thinking: Vec::new(),
            text,
            // Tool calls are handled separately in the handler
            // since they need to be executed and recorded with results
            tool_calls: Vec::new(),
        }
    }
}
