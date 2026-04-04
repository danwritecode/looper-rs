use crate::types::turn::{ThinkingBlock, TurnStep};
use async_anthropic::types::{CreateMessagesResponse, MessageContent};

impl From<CreateMessagesResponse> for TurnStep {
    fn from(response: CreateMessagesResponse) -> Self {
        let mut thinking = Vec::new();
        let mut text = None;
        let tool_calls = Vec::new();

        if let Some(content) = response.content {
            for block in content {
                match block {
                    MessageContent::Thinking(t) => {
                        thinking.push(ThinkingBlock {
                            content: t.thinking,
                        });
                    }
                    MessageContent::Text(t) => {
                        text = Some(t.text);
                    }
                    // ToolUse blocks are handled separately in the handler
                    // since they need to be executed and recorded with results
                    _ => {}
                }
            }
        }

        TurnStep {
            thinking,
            text,
            tool_calls,
        }
    }
}
