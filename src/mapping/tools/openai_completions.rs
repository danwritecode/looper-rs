use crate::types::LooperToolDefinition;
use async_openai::types::chat::{ChatCompletionTool, FunctionObjectArgs};

impl From<LooperToolDefinition> for ChatCompletionTool {
    fn from(value: LooperToolDefinition) -> Self {
        ChatCompletionTool {
            function: FunctionObjectArgs::default()
                .name(value.name)
                .description(value.description)
                .parameters(value.parameters)
                .build()
                .expect("Failed to build FunctionObjectArgs from LooperTool"),
        }
    }
}
