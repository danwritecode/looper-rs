# loopin-rs

A super lightweight agentic loop (similar to Claude Code or Codex CLI) written in Rust. Allowing for programmatic access to an agentic system without the bloat of spawning CLI processes through agent SDKs.

## Features

- Clear separation of concerns between the UI and the agentic loop and handlers
- Agentic loop with tool use (read/write files, grep, find, list directory)
- Streaming responses with reasoning token support
- Terminal UI with spinner and themed output

## Setup

```sh
cp .env.example .env
# Add your OPENAI_API_KEY to .env
```

## Usage

```sh
cargo run
```
