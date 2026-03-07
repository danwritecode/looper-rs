use async_anthropic::types::Tool;
use crate::types::LooperToolDefinition;

impl From<LooperToolDefinition> for Tool {
    fn from(value: LooperToolDefinition) -> Self {
        Tool::Custom {
            name: value.name,
            description: Some(value.description),
            input_schema: value.parameters,
        }
    }
}
