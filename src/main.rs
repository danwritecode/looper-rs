use std::{error::Error, io::{self, Write}, sync::Arc};

use indicatif::ProgressBar;
use tokio::sync::{Notify, mpsc};

use crate::{looper::Looper, theme::Theme, types::LooperToInterfaceMessage};

mod looper;
mod mapping;
mod services;
mod theme;
mod tools;
mod types;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let (tx, mut rx) = mpsc::channel(10000);
    let mut looper = Looper::new(tx)?;
    let turn_done = Arc::new(Notify::new());
    let turn_done_tx = turn_done.clone();

    tokio::spawn(async move{
        let theme = Theme::default();
        let mut spinner: Option<ProgressBar> = None;
        let mut thinking_buf = String::new();

        while let Some(message) = rx.recv().await {
            if let Some(sp) = spinner.take() { sp.finish_and_clear(); }

            match message {
                LooperToInterfaceMessage::Assistant(m) => {
                    print!("{}", m);
                    io::stdout().flush().ok();
                },
                LooperToInterfaceMessage::Thinking(m) => {
                    if thinking_buf.is_empty() {
                        spinner = Some(theme.thinking_spinner());
                    }
                    thinking_buf.push_str(&m);
                },
                LooperToInterfaceMessage::ThinkingComplete => {
                    if !thinking_buf.is_empty() {
                        println!("{}", theme.thinking.apply_to(&thinking_buf));
                        thinking_buf.clear();
                    }
                },
                LooperToInterfaceMessage::ToolCall(name) => {
                    spinner = Some(theme.tool_spinner(&name));
                },
                LooperToInterfaceMessage::TurnComplete => {
                    println!("\n{}", theme.separator_line());
                    turn_done_tx.notify_one();
                }
            }
        }
    });

    let theme = Theme::default();
    loop {
        print!("{}", theme.prompt());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        looper.send(&input).await?;
        turn_done.notified().await;
    }
}
