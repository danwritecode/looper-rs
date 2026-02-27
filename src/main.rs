use std::{error::Error, io::{self, Write}, time::Duration};

use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::mpsc;

use crate::{looper::Looper, types::LooperToInterfaceMessage};

mod looper;
mod services;
mod tools;
mod types;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let (tx, mut rx) = mpsc::channel(10000);
    let mut looper = Looper::new(tx)?;

    tokio::spawn(async move{
        let mut spinner: Option<ProgressBar> = None;

        while let Some(message) = rx.recv().await {
            if let Some(sp) = spinner.take() { sp.finish_and_clear(); }

            match message {
                LooperToInterfaceMessage::Assistant(m) => {
                    print!("{}", m);
                    io::stdout().flush().ok();
                },
                LooperToInterfaceMessage::ToolCall(name) => {
                    spinner = Some(tool_spinner(&name));
                },
                LooperToInterfaceMessage::TurnComplete => {
                    println!("");
                }
            }
        }
    });

    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        looper.send(&input).await?;
    }
}


fn tool_spinner(name: &str) -> ProgressBar {
    let sp = ProgressBar::new_spinner();
    sp.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["▖", "▘", "▝", "▗", "▚", "▞", ""])
    );
    sp.set_message(name.to_string());
    sp.enable_steady_tick(Duration::from_millis(80));
    sp
}
