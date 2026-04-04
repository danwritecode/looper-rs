use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    tools::{LooperTool, LooperTools},
    types::LooperToolDefinition,
};

pub struct EmptyToolSet;

#[async_trait]
impl LooperTools for EmptyToolSet {
    async fn get_tools(&self) -> Vec<LooperToolDefinition> {
        vec![]
    }

    async fn add_tool(&mut self, _tool: Arc<dyn LooperTool>) {
        panic!("Can't add a tool to an empty Tool Set. Create a valid Tool Set to continue")
    }

    async fn run_tool(&self, name: String, _args: Value) -> Value {
        json!({"error": format!("Unknown function: {}", name)})
    }
}
