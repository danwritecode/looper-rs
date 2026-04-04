use crate::types::LooperToolDefinition;
use async_anthropic::types::Tool;

impl From<LooperToolDefinition> for Tool {
    fn from(value: LooperToolDefinition) -> Self {
        Tool::Custom {
            name: value.name,
            description: Some(value.description),
            input_schema: value.parameters,
        }
    }
}
