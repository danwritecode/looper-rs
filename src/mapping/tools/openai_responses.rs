use crate::types::LooperToolDefinition;
use async_openai::types::responses::{FunctionTool, FunctionToolArgs};

impl From<LooperToolDefinition> for FunctionTool {
    fn from(value: LooperToolDefinition) -> Self {
        FunctionToolArgs::default()
            .name(value.name)
            .description(value.description)
            .parameters(value.parameters)
            .build()
            .expect("Failed to build FunctionTool from LooperToolDefinition")
    }
}
