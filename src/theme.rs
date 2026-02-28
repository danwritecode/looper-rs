use std::time::Duration;

use console::Style;
use indicatif::{ProgressBar, ProgressStyle};

pub struct Theme {
    pub thinking: Style,
    pub separator: Style,
    pub tool_spinner: Style,
    pub prompt: Style,
    pub greeting: Style,
}

impl Theme {
    pub fn default() -> Self {
        Theme {
            thinking: Style::new().green().dim().italic(),
            separator: Style::new().green().dim(),
            tool_spinner: Style::new().yellow(),
            prompt: Style::new().green().bold(),
            greeting: Style::new().green().bold(),
        }
    }

    pub fn greeting(&self) -> String {
        format!("{}\n", self.greeting.apply_to("\u{1F980} Welcome to Looper.rs"))
    }

    pub fn prompt(&self) -> String {
        self.prompt.apply_to("> ").to_string()
    }

    pub fn separator_line(&self) -> String {
        self.separator.apply_to("────────────────────────────────").to_string()
    }

    pub fn tool_spinner(&self, name: &str) -> ProgressBar {
        let sp = ProgressBar::new_spinner();
        sp.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["▖", "▘", "▝", "▗", "▚", "▞", ""])
        );
        sp.set_message(self.tool_spinner.apply_to(name).to_string());
        sp.enable_steady_tick(Duration::from_millis(80));
        sp
    }

    pub fn thinking_spinner(&self) -> ProgressBar {
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
