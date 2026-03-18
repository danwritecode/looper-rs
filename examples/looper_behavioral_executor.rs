use std::error::Error;
use std::io::{self, Read};

use serde::{Deserialize, Serialize};

use looper::{looper::Looper, types::Handlers};

#[derive(Debug, Deserialize)]
struct Request {
    provider: String,
    model: Option<String>,
    instructions: String,
    user_turns: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Response {
    final_text: String,
}

fn default_model(provider: &str) -> &'static str {
    match provider {
        "openai" => "gpt-5.4",
        "anthropic" => "claude-sonnet-4-6",
        "gemini" => "gemini-2.5-flash",
        _ => "gpt-5.4",
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    let request: Request = serde_json::from_str(&raw)?;

    if request.user_turns.is_empty() {
        return Err("user_turns must not be empty".into());
    }

    let provider = request.provider.trim().to_lowercase();
    let model = request
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_model(&provider))
        .to_string();

    let handler = match provider.as_str() {
        "openai" => Handlers::OpenAIResponses(&model),
        "anthropic" => Handlers::Anthropic(&model),
        "gemini" => Handlers::Gemini(&model),
        other => {
            return Err(
                format!("unsupported provider for Looper behavioral executor: {other}").into(),
            );
        }
    };

    let mut looper = Looper::builder(handler)
        .instructions(request.instructions)
        .build()
        .await?;

    let mut final_text = String::new();
    for turn in request.user_turns {
        let result = looper.send(&turn).await?;
        final_text = result.final_text.unwrap_or_default();
    }

    println!("{}", serde_json::to_string(&Response { final_text })?);

    Ok(())
}
