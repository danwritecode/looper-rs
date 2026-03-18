---
name: looper-rs
description: |
  This skill should be used when working on the looper-rs Rust crate (a headless agentic loop library).
  Use it for:
  - Adding or modifying providers (OpenAI, Anthropic, Gemini)
  - Implementing new tool types or toolsets
  - Changing handler logic (streaming/non-streaming)
  - Updating examples
  - Refactoring message history or turn mapping
---

# Looper Rs

## Overview

A lightweight, UI-agnostic "headless" agentic loop library in Rust. Provides:
- **Non-streaming API** (`Looper`): returns complete `TurnResult` after the loop finishes.
- **Streaming API** (`LooperStream`): forwards events (text, thinking, tool calls) over an `mpsc` channel as they arrive.
- **Multi-provider support**: OpenAI Completions, OpenAI Responses, Anthropic, Gemini.
- **Dynamic tool injection** via `LooperTools` trait.
- **Sub-agent delegation** with `spawn_sub_agent` tool.

Repository: https://github.com/anomalyco/looper-rs

## Core Architecture

### 1. Entry Points

| API | Type | Build method | Primary use |
|-----|------|--------------|-------------|
| `Looper` | Non-streaming | `Looper::builder(...).build().await?` | Background tasks, no live UI needed |
| `LooperStream` | Streaming | `LooperStream::builder(...).build().await?` | CLI/web UIs needing live deltas |

Builder methods available for both:
- `.tools(Box<dyn LooperTools>)` — register toolset
- `.instructions(impl Into<String>)` — system prompt
- `.sub_agent(Looper)` — attach nested agent
- `.message_history(MessageHistory)` — resume prior conversation

`LooperStream` also supports:
- `.interface_sender(Sender<LooperToInterfaceMessage>)` — required for UI events
- `.buffered_output()` — smooth char-by-char rendering via a 5ms drain interval

### 2. Provider Handlers

Location: `src/services/handlers/`

Each provider has two handlers:
- Streaming: `anthropic.rs`, `openai_completions.rs`, `openai_responses.rs`, `gemini.rs`
- Non-streaming: `anthropic_non_streaming.rs`, `openai_completions_non_streaming.rs`, `openai_responses_non_streaming.rs`, `gemini_non_streaming.rs`

Handlers implement recursive inner loops:
1. Send request to LLM
2. Stream/collect assistant tokens, thinking tokens, tool calls
3. Execute tools concurrently (`JoinSet`)
4. Append tool results to history
5. Recurse until no more tool calls

### 3. Types and Contracts

- `src/types/handlers.rs`: `Handlers` enum (OpenAICompletions, OpenAIResponses, Anthropic, Gemini), `MessageHistory` (serialized Vec<Message> or ResponseId)
- `src/types/messages.rs`: `HandlerToLooperMessage`, `LooperToInterfaceMessage`, `HandlerToLooperToolCallRequest`
- `src/types/turn.rs`: `TurnResult`, `TurnStep`, `ToolCallRecord`, `ThinkingBlock`
- `src/types/tool.rs`: `LooperToolDefinition` (builder-style API)
- `src/tools/mod.rs`: `LooperTool` (single tool) and `LooperTools` (toolset) traits
- `src/tools/sub_agent.rs`: `SubAgentTool` wraps a `Looper` and exposes `spawn_sub_agent`
- `src/tools/empty.rs`: `EmptyToolSet` — fallback when no tools provided

### 4. Mapping Layer

- `src/mapping/tools/*.rs`: Map `LooperToolDefinition` into provider-specific tool schemas
- `src/mapping/turn/*.rs`: Map provider responses into internal `TurnStep`

### 5. System Prompt

Location: `prompts/system_prompt.txt`

Rendered with Tera: supports `{{ instructions }}` and conditional `{% if sub_agent %}`. Keep additions concise; the prompt intentionally limits scope and meta chatter.

## Critical Conventions and Gotchas

### 1. Sub-agent Tool Set Constraint

Sub-agent **must** use the *exact same* `LooperTools` implementation as the parent. This is not enforced at the type level. Violation causes runtime mismatches in tool availability.

Location: noted in `src/looper.rs:48` and `src/looper_stream.rs:55`

### 2. EmptyToolSet Panics on add_tool

If you intend to add tools or sub-agents, you cannot use the default `EmptyToolSet`. Provide a real `LooperTools` implementation in the builder.

Location: `src/tools/empty.rs:16`

### 3. Tool Execution Concurrency

Tools execute concurrently via `tokio::task::JoinSet`. Tool implementations must be `Send + Sync` and must not assume single-threaded execution.

### 4. Environment Setup

Required env vars (depending on provider):
- `OPENAI_API_KEY` (Completions + Responses)
- `ANTHROPIC_API_KEY`
- `GEMINI_API_KEY` or `GOOGLE_API_KEY` (code checks both)

Note: `.env.example` currently only lists `OPENAI_API_KEY`, but the code and README support all three providers.

Setup:
```bash
cp .env.example .env
# Edit .env and add keys for the providers you intend to use
```

### 5. No Dedicated Test Suite

Examples serve as the primary executable verification surface:
- `cargo run --example cli` — streaming demo
- `cargo run --example cli_non_streaming` — non-streaming demo

If adding features, extend examples or add new ones under `examples/`.

### 6. Provider/Model Validity Not Guarded

The `Handlers` enum accepts any model string. Invalid models fail at runtime with provider errors. There is no compile-time or builder-time validation yet.

## File Reference Quick Map

| Purpose | Path |
|---------|------|
| Library entry | `src/lib.rs` |
| Non-streaming looper | `src/looper.rs` |
| Streaming looper | `src/looper_stream.rs` |
| Handler traits | `src/services/chat_handler.rs`, `src/services/chat_handler_streaming.rs` |
| Provider handlers | `src/services/handlers/` |
| Tool traits | `src/tools/mod.rs` |
| Sub-agent tool | `src/tools/sub_agent.rs` |
| Empty fallback | `src/tools/empty.rs` |
| Message types | `src/types/messages.rs` |
| Handler/provider types | `src/types/handlers.rs` |
| Turn result types | `src/types/turn.rs` |
| Tool definition | `src/types/tool.rs` |
| System prompt | `prompts/system_prompt.txt` |
| Example streaming | `examples/cli.rs` |
| Example non-streaming | `examples/cli_non_streaming.rs` |
| Crate manifest | `Cargo.toml` |

## Developer Workflow

When modifying this crate:

1. **Changing a provider handler**: Keep streaming and non-streaming variants in sync. They share logic patterns (recursive loop, tool execution, history management).

2. **Adding a new provider**: Create both streaming and non-streaming handlers in `src/services/handlers/`, implement the respective traits, and add the variant to `Handlers` in `src/types/handlers.rs`. Update mapping layers in `src/mapping/`.

3. **Adding tools**: Implement `LooperTool`, add to your `LooperTools` implementation, and ensure the toolset is passed to both parent and sub-agent builders.

4. **Modifying system prompt**: Preserve the terse, scope-limiting style. Use Tera conditionals for optional sections (instructions, sub_agent).

5. **Running locally**: Ensure `.env` has the required keys, then use the examples to verify behavior.
