# looper-rs

A headless, UI-agnostic agentic loop for Rust. Provides streaming and non-streaming APIs with multi-provider LLM support (OpenAI, Anthropic, Gemini).

## Where to Start

- **Quick usage**: [`README.md`](./README.md)
- **Architecture**: [`docs/core-agent-loop-architecture.md`](./docs/core-agent-loop-architecture.md)
- **Example integration**: [`examples/cli.rs`](./examples/cli.rs)

## Repository Map

```
src/
├── lib.rs                    # Public module exports
├── looper.rs                 # Non-streaming API: Looper, LooperBuilder
├── looper_stream.rs          # Streaming API: LooperStream, LooperStreamBuilder
├── types/                    # Shared contracts
│   ├── messages.rs           # MessageHistory, event types
│   ├── tool.rs               # LooperToolDefinition
│   ├── handlers.rs           # Handlers enum, provider selection
│   └── turn.rs               # TurnResult, TurnStep
├── services/                 # Provider execution layer
│   ├── chat_handler.rs       # ChatHandler trait (non-streaming)
│   ├── chat_handler_streaming.rs  # StreamingChatHandler trait
│   └── handlers/             # Provider implementations
│       ├── openai_*.rs       # OpenAI Completions/Responses
│       ├── anthropic*.rs     # Anthropic handlers
│       └── gemini*.rs        # Gemini handlers
├── mapping/                  # SDK translation layer
│   └── tools/                # Tool definition mappings per provider (src/mapping/tools/)
└── tools/                    # Tool contract and built-ins
    ├── mod.rs                # LooperTool, LooperTools traits
    ├── empty.rs              # EmptyToolSet fallback
    └── sub_agent.rs          # SubAgentTool for delegation

docs/
├── core-agent-loop-architecture.md   # Control flow and invariants
├── services-layer.md                 # Handler contracts
├── tools-layer.md                    # Tool registry patterns
└── mapping-layer.md                  # SDK translation details

examples/
├── cli.rs                  # Streaming CLI with tools
└── cli_non_streaming.rs    # Simple blocking example

prompts/
└── system_prompt.txt       # Tera template for agent instructions
```

## Build, Test, Run

```bash
# Build
cargo build

# Run tests
cargo test

# Run examples
cargo run --example cli
cargo run --example cli_non_streaming

# Lint/format (enforced via cargo-husky prepush hook)
cargo clippy
cargo fmt
```

## Architecture Boundaries

| Layer | Responsibility | Key Files |
|-------|---------------|-----------|
| **Entry** | Builder API, public surface | `looper.rs`, `looper_stream.rs` |
| **Services** | Provider SDK interaction | `services/handlers/*.rs` |
| **Mapping** | Tool schema translation | `mapping/tools/*.rs` |
| **Tools** | Tool execution registry | `tools/mod.rs` |
| **Types** | Shared contracts | `types/*.rs` |

## Provider Handlers

All handlers follow the same recursion pattern: send request → parse response → execute tools → recurse until no tool calls remain.

| Provider | Streaming | Non-streaming |
|----------|-----------|---------------|
| OpenAI Completions | `OpenAICompletionsHandler` | `OpenAICompletionsNonStreamingHandler` |
| OpenAI Responses | `OpenAIResponsesHandler` | `OpenAIResponsesNonStreamingHandler` |
| Anthropic | `AnthropicHandler` | `AnthropicNonStreamingHandler` |
| Gemini | `GeminiHandler` | `GeminiNonStreamingHandler` |

## Key Docs

| Doc | Purpose |
|-----|---------|
| [`docs/core-agent-loop-architecture.md`](./docs/core-agent-loop-architecture.md) | Turn lifecycle, event flow, invariants |
| [`docs/services-layer.md`](./docs/services-layer.md) | Handler contracts and provider patterns |
| [`docs/tools-layer.md`](./docs/tools-layer.md) | Tool trait design and registry patterns |
| [`docs/mapping-layer.md`](./docs/mapping-layer.md) | SDK translation layer |

## Change Rules

1. **New provider**: Add handler in `services/handlers/`, implement both streaming + non-streaming traits, add tool mapping in `mapping/tools/`
2. **New tool**: Implement `LooperTool` trait, register in your `LooperTools` impl (see `examples/cli.rs` for pattern)
3. **API changes**: Update both `Looper` and `LooperStream` builders if shared options affected
4. **History models**: `MessageHistory::Messages` (OpenAI Completions, Anthropic, Gemini) vs `MessageHistory::ResponseId` (OpenAI Responses) — do not mix

## Validation Checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy` clean
- [ ] `cargo fmt` clean
- [ ] Examples compile: `cargo build --examples`
- [ ] New paths added to this file if user-facing
