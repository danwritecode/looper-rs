# looper-rs

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


source code here. still need to productionize this a bit but the core loop and separation of concerns are baked in and ready to be extended.


looper-rs

an agentic loop without the bloat of claude code/codex SDK spawning processes of itself on your server.

designed to be super light way and extensible with any UI layer for use in agentic user applications
