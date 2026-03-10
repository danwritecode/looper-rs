use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    tools::LooperTools,
    types::{LooperToolDefinition, MessageHistory, turn::TurnResult},
};

#[async_trait]
pub trait ChatHandler: Send + Sync {
    async fn send_message(
        &mut self,
        message_history: Option<MessageHistory>,
        message: &str,
        tools_runner: Option<&Arc<Mutex<dyn LooperTools>>>,
    ) -> Result<TurnResult>;

    fn set_tools(&mut self, tools: Vec<LooperToolDefinition>);
}
