use std::{error::Error, io::{self, Write}, sync::Arc, time::Duration};

use console::{Style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::{Notify, mpsc};

use loopin_rs::{looper::Looper, tools::LooperTools, types::{Handlers, LooperToInterfaceMessage}};


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let term = Term::stdout();
    term.clear_screen()?;
    let theme = Theme::default();

    let handler = Handlers::OpenAIResponses;
    let tools = LooperTools::new();
    let (tx, mut rx) = mpsc::channel(10000);

    let mut looper = Looper::new(handler, tools, tx)?;
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

    loop {
        print!("{}", theme.prompt());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        looper.send(&input).await?;
        turn_done.notified().await;
    }
}

struct Theme {
    thinking: Style,
    separator: Style,
    tool_spinner: Style,
    prompt: Style,
    greeting: Style,
}

impl Theme {
    fn default() -> Self {
        Theme {
            thinking: Style::new().green().dim().italic(),
            separator: Style::new().green().dim(),
            tool_spinner: Style::new().yellow(),
            prompt: Style::new().green().bold(),
            greeting: Style::new().green().bold(),
        }
    }

    fn greeting(&self) -> String {
        format!("{}\n", self.greeting.apply_to("\u{1F980} Welcome to Looper.rs"))
    }

    fn prompt(&self) -> String {
        self.prompt.apply_to("> ").to_string()
    }

    fn separator_line(&self) -> String {
        self.separator.apply_to("────────────────────────────────").to_string()
    }

    fn tool_spinner(&self, name: &str) -> ProgressBar {
        let sp = ProgressBar::new_spinner();
        sp.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["▖", "▘", "▝", "▗", "▚", "▞", ""])
        );
        sp.set_message(self.tool_spinner.apply_to(name).to_string());
        sp.enable_steady_tick(Duration::from_millis(80));
        sp
    }

    fn thinking_spinner(&self) -> ProgressBar {
        let sp = ProgressBar::new_spinner();
        sp.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["·  ", "·· ", "···", " ··", "  ·", "   "])
                .template("{spinner} thinking")
                .unwrap()
        );
        sp.enable_steady_tick(Duration::from_millis(200));
        sp
    }
}
