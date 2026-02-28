use async_openai::types::chat::{ChatCompletionTool, FunctionObjectArgs};
use crate::types::LooperToolDefinition;

impl From<LooperToolDefinition> for ChatCompletionTool {
    fn from(value: LooperToolDefinition) -> Self {
        ChatCompletionTool {
            function: FunctionObjectArgs::default()
                .name(value.name)
                .description(value.description)
                .parameters(value.parameters)
                .build()
                .expect("Failed to build FunctionObjectArgs from LooperTool")
        }
    }
}
