pub mod sub_agent;
use std::sync::Arc;

pub use sub_agent::*;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::types::LooperToolDefinition;

#[async_trait]
pub trait LooperTool: Send + Sync {
    async fn execute(&self, args: &Value) -> Value;
    fn tool(&self) -> LooperToolDefinition;
    fn get_tool_name(&self) -> String;
}

#[async_trait]
pub trait LooperTools: Send + Sync {
    async fn get_tools(&self) -> Vec<LooperToolDefinition>;
    async fn add_tool(&mut self, tool: Arc<Mutex<dyn LooperTool>>);
    async fn run_tool(&self, name: &str, args: Value) -> Value;
}
