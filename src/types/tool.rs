use async_openai::types::chat::{ChatCompletionTool, FunctionObjectArgs};
use serde_json::{Value, json};

pub struct LooperToolDefinition {
    name: String,
    description: String,
    parameters: Value
}

impl Default for LooperToolDefinition {
    fn default() -> Self {
        LooperToolDefinition { name: "".to_string(), description: "".to_string(), parameters: json!({}) }
    }
}

impl LooperToolDefinition {
    pub fn name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    pub fn description(mut self, description: &str) -> Self {
        self.description = description.to_string();
        self
    }

    pub fn paramters(mut self, parameters: Value) -> Self {
        self.parameters = parameters;
        self
    }
}

// TODO: As this expands, separate these into separate tool files and separate mapping files.

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
