use serde_json::{Value, json};

#[derive(Debug)]
pub struct LooperToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value
}

impl Default for LooperToolDefinition {
    fn default() -> Self {
        LooperToolDefinition { name: "".to_string(), description: "".to_string(), parameters: json!({}) }
    }
}

impl LooperToolDefinition {
    pub fn set_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    pub fn set_description(mut self, description: &str) -> Self {
        self.description = description.to_string();
        self
    }

    pub fn set_paramters(mut self, parameters: Value) -> Self {
        self.parameters = parameters;
        self
    }
}
