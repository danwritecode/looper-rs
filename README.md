# looper-rs

## Demo

[Demo video (MP4)](./assets/Demo.mp4)

A very barebones, lightweight, headless agentic loop made to be plugged into any UI chat interface (CLI, web, desktop, etc).

The purpose of this is to avoid having to us Claude Code/Codex CLI which require sub processes of themselves to be spawned per user chat session. This become unscalable quickly with even just a few dozen sessions.

This tool is *not* meant to be as robust as Claude Code/Codex. It's meant to be a lighter weight, more practical solution to their heavy SDKs.

### What is a "headless" Agent Loop?
It's just like using Claude Code or Codex SDK but without the underlying process bloat. It doesn't care what the interface is, you can render messages however you want.

## Features

- Streaming and non-streaming modes
- Multi-provider support (OpenAI Completions, OpenAI Responses, Anthropic, Gemini)
- Agentic loop with concurrent tool calling
- Dynamic tool injection
- Sub-agent delegation
- Buffered output mode for smooth char-by-char rendering
- UI agnostic event stream (assistant text, thinking, tool calls, turn completion)

## Usage

### Non-Streaming

Returns the complete response after the agent loop finishes. Good for background tasks or when you don't need live output.

```rust
let mut looper = Looper::builder(Handlers::OpenAIResponses("gpt-5.4"))
    .instructions("Be helpful.")
    .build()
    .await?;

let result = looper.send("What files are in this directory?").await?;
println!("{}", result.final_text.unwrap());
```

### Streaming

Forwards events (text deltas, thinking, tool calls) over an `mpsc` channel as they arrive. `build()` returns a `(LooperStream, Receiver)` tuple — wire up the receiver to your UI.

```rust
let (mut looper, mut rx) = LooperStream::builder(Handlers::Anthropic("claude-sonnet-4-6"))
    .tools(tools)
    .instructions("Be helpful.")
    .build()
    .await?;

// consume events in a separate task
tokio::spawn(async move {
    while let Some(msg) = rx.recv().await {
        match msg {
            LooperToInterfaceMessage::Assistant(text) => print!("{text}"),
            LooperToInterfaceMessage::Thinking(text)  => print!("{text}"),
            LooperToInterfaceMessage::ToolCall(name)   => println!("[tool: {name}]"),
            LooperToInterfaceMessage::TurnComplete     => println!("\n---"),
            _ => {}
        }
    }
});

looper.send("Read the README").await?;
```

### Builder Options

Both `Looper` and `LooperStream` share these builder methods:

| Method | Description |
|---|---|
| `.tools(Box<dyn LooperTools>)` | Register tools the agent can call |
| `.instructions(impl Into<String>)` | Set a system prompt |
| `.sub_agent(Looper)` | Attach a sub-agent (must have the same tools) |
| `.message_history(MessageHistory)` | Resume from prior conversation state |

`LooperStream` also supports:

| Method | Description |
|---|---|
| `.buffered_output()` | Smooth char-by-char text rendering instead of raw deltas |

### Supported Handlers Examples

You can pass in any model text you want. Be aware, that some features are not supported by all models. For example, Haiku models don't support adaptive thinking.

A future TODO here is to provide more options that are "provider and model" aware so that the caller cannot pass in an invalid config.

| Variant | Example model |
|---|---|
| `Handlers::OpenAICompletions(model)` | `"gpt-5.4"` |
| `Handlers::OpenAIResponses(model)` | `"gpt-5.4"` |
| `Handlers::Anthropic(model)` | `"claude-sonnet-4-6"` |
| `Handlers::Gemini(model)` | `"gemini-2.5-flash"` |


## Architecture

```mermaid
sequenceDiagram
    participant UI
    participant Looper
    participant Handler
    participant LLM
    participant Tools

    UI->>Looper: user input
    Looper->>Handler: send_message()

    loop Agent Loop (until state = Done)
        Handler->>LLM: stream request
        LLM-->>Handler: thinking tokens
        Handler-->>Looper: Thinking(text)
        Looper-->>UI: Thinking(text)

        LLM-->>Handler: assistant tokens
        Handler-->>Looper: Assistant(text)
        Looper-->>UI: Assistant(text)

        LLM-->>Handler: tool calls
        Handler-->>Looper: ToolCallRequest(name, args, oneshot_tx)
        Looper-->>UI: ToolCall(name)
        Looper->>Tools: run_tool(name, args)
        Tools-->>Looper: result
        Looper->>Handler: tool result via oneshot

        Note over Handler: recursive inner_send_message()
    end

    Handler-->>Looper: TurnComplete
    Looper-->>UI: TurnComplete
```

## Setup

```sh
cp .env.example .env
# Add your API keys to .env (OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY)
```


### Running Examples

```sh
cargo run --example cli              # streaming
cargo run --example cli_non_streaming
```
