use anyhow::Result;
use async_trait::async_trait;
use crate::types::LooperToolDefinition;

#[async_trait]
pub trait ChatHandler {
    async fn send_message(&mut self, message: &str) -> Result<()>;
    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>);
    fn set_continue(&mut self);
}
