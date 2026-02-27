use anyhow::Result;
use crate::services::ChatHandler;

#[derive(Debug)]
pub enum LooperResponse {
    Assistant(String),
    ToolCall(String)
}

#[derive(Debug)]
pub enum LooperState {
    Continue(String),
    Done
}


pub struct Looper {
    handler: Box<dyn ChatHandler>,
}

impl Looper {
    pub fn new(handler: Box<dyn ChatHandler>) -> Self {
        Looper { handler }
    }

    pub async fn send(&mut self, message: &str) -> Result<()> {
        // handle user message
        self.handler.add_message(message)?;
        self.handler.send_message().await?;

        Ok(())
    }
}

