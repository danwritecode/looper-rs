use std::{error::Error, io::{self, Write}};
use tokio::sync::mpsc;

use crate::{looper::{Looper, LooperResponse}, services::OpenAIChatHandler};

mod looper;
mod services;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let (tx, mut rx) = mpsc::channel(10000);

    let handler = Box::new(OpenAIChatHandler::new(tx)?);
    let mut looper = Looper::new(handler);

    tokio::spawn(async move{
        while let Some(message) = rx.recv().await {
            match message {
                LooperResponse::Assistant(m) => {
                    if m == "<END>" {
                        println!("");
                    } else {
                        print!("{}", m);
                        io::stdout().flush().ok();
                    }
                },
                LooperResponse::ToolCall(name) => {
                    println!("Calling: {}", name);
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
