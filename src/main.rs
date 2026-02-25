use std::{error::Error, io::{self}};
use tokio::sync::mpsc;

use crate::looper::{Looper, LooperResponse};

mod looper;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let (tx, mut rx) = mpsc::channel(10000);

    let mut looper = Looper::new(tx);

    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        looper.send(&input).await?;

        while let Some(message) = rx.recv().await {
            match message {
                LooperResponse::Assistant(m) => {
                    if m == "<END>" {
                        break;
                    }

                    print!("{}", m);
                },
                LooperResponse::ToolCall => {
                    println!("Calling tool...");
                }
            }
        }

        println!("");
    }
}
