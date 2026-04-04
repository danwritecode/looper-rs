use std::sync::Arc;

use crate::{
    tools::LooperTools,
    types::{LooperToolDefinition, MessageHistory},
};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait StreamingChatHandler: Send + Sync {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str,
        tools_runner: Arc<dyn LooperTools>,
    ) -> Result<MessageHistory>;

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>);
}
