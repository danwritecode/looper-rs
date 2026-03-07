pub mod set_agent_loop_state;
pub use set_agent_loop_state::*;

use async_trait::async_trait;
use serde_json::Value;

use crate::types::LooperToolDefinition;

#[async_trait]
pub trait LooperTool: Send + Sync {
    async fn execute(&self, args: &Value) -> Value;
    fn tool(&self) -> LooperToolDefinition;
}

#[async_trait]
pub trait LooperTools: Send + Sync {
    fn get_tools(&self) -> Vec<LooperToolDefinition>;
    async fn run_tool(&self, name: &str, args: Value) -> Value;
}
