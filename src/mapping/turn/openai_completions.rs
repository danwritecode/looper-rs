use crate::types::turn::TurnStep;
use async_openai::types::chat::{ChatChoice, FinishReason};

impl From<ChatChoice> for TurnStep {
    fn from(choice: ChatChoice) -> Self {
        let text = choice.message.content;
        let has_tool_calls = matches!(choice.finish_reason, Some(FinishReason::ToolCalls));

        TurnStep {
            thinking: Vec::new(),
            text,
            // Tool calls are handled separately in the handler
            // since they need to be executed and recorded with results
            tool_calls: if has_tool_calls {
                Vec::new()
            } else {
                Vec::new()
            },
        }
    }
}
