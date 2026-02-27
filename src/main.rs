use std::{error::Error, io::{self, Write}};

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
        while let Some(message) = rx.recv().await {
            match message {
                LooperToInterfaceMessage::Assistant(m) => {
                    print!("{}", m);
                    io::stdout().flush().ok();
                },
                LooperToInterfaceMessage::ToolCall(name) => {
                    println!("Calling: {}", name);
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
