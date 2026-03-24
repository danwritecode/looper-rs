---
title: "Core Agent Loop Architecture"
when_to_read:
  - "When you need to understand how a user message becomes model output, tool calls, and updated history."
  - "When you are adding a new provider handler, changing tool execution, or wiring a UI to streamed events."
summary: "`looper-rs` is a provider-agnostic agent loop with two entry points: `Looper` for complete turn results and `LooperStream` for incremental UI events. This page explains the shared control flow, where providers diverge, and which invariants matter when you extend the crate or debug a broken turn."
ontology_relations:
  - relation: "depends_on"
    target: "src/types"
    note: "The shared message, tool, handler, and turn types define the contracts described here."
  - relation: "depends_on"
    target: "prompts/system_prompt.txt"
    note: "Every handler receives the rendered system prompt built from this template."
  - relation: "feeds"
    target: "examples/cli.rs"
    note: "The streaming CLI consumes the event semantics explained in this document."
  - relation: "feeds"
    target: "examples/cli_non_streaming.rs"
    note: "The non-streaming CLI displays the `TurnResult` shape explained here."
---

# Purpose

Explain the crate's real execution model: how builders create provider-specific handlers, how a turn recurses through tool calls, how history is persisted, and where the streaming and non-streaming APIs differ.

# Scope

This page covers the hot path through:

- `src/looper.rs`
- `src/looper_stream.rs`
- `src/services/chat_handler*.rs`
- `src/services/handlers/*`
- `src/tools/*`
- `src/types/*`
- `src/mapping/tools/*`

It does not try to document each upstream SDK. It also does not treat `src/mapping/turn/*` as core runtime behavior, because the current handlers build `TurnStep` values directly instead of using those conversions.

# Main content

## Core structure

There are four layers in the crate:

1. Entry points:
   `Looper` is the non-streaming API. It returns a `TurnResult` after the full agent loop finishes.
   `LooperStream` is the streaming API. It returns updated `MessageHistory` and emits `LooperToInterfaceMessage` events during the turn.
2. Provider handlers:
   `ChatHandler` and `StreamingChatHandler` are the provider-agnostic traits.
   Concrete handlers in `src/services/handlers/*` implement the actual SDK calls for OpenAI Completions, OpenAI Responses, Anthropic, and Gemini.
3. Tool execution:
   `LooperTool` defines one callable tool.
   `LooperTools` is the tool registry and execution surface the handlers call back into.
4. Shared state:
   `MessageHistory` stores conversation continuity.
   `TurnResult` and `TurnStep` are the non-streaming reconstruction of what happened inside a turn.

## Flow / behavior

1. Build time

   `Looper::builder(...)` and `LooperStream::builder(...)` choose a provider enum from `Handlers` and instantiate the matching handler.

   Both builders render `prompts/system_prompt.txt` through Tera. That template optionally injects:

   - caller-supplied instructions
   - sub-agent guidance when `.sub_agent(...)` was configured

   If a tool registry was supplied, the builder passes provider-specific tool definitions into the handler. The conversion from `LooperToolDefinition` into each SDK's tool schema happens in `src/mapping/tools/*`.

   If `.sub_agent(...)` was set and a mutable tool registry exists, the builder adds `SubAgentTool` before handing tool definitions to the handler.

2. Turn start

   `send(...)` on either entry point passes three things into the selected handler:

   - prior `MessageHistory`
   - the new user message
   - a shared `Arc<dyn LooperTools>` runner

   The handler restores provider-specific history before appending the new user input.

3. Model request

   Each handler sends the current conversation plus tool definitions to the provider.

   Provider-specific continuity differs:

   - OpenAI Responses uses `MessageHistory::ResponseId` and relies on server-side response chaining via `previous_response_id`.
   - OpenAI Completions, Anthropic, and Gemini use `MessageHistory::Messages`, which is a serialized SDK message vector stored locally as `serde_json::Value`.

4. Model output handling

   The handlers inspect the returned response for:

   - assistant text
   - thinking or reasoning content
   - tool calls

   Streaming handlers emit `HandlerToLooperMessage` events as tokens or blocks arrive. `LooperStream` optionally forwards those events to the UI and can buffer assistant text for smoother character-by-character rendering.

   Non-streaming handlers accumulate one `TurnStep` per provider round-trip. A step may contain:

   - zero or more thinking blocks
   - optional assistant text
   - zero or more tool call records with both args and results

5. Tool execution

   When a provider returns tool calls, the handler executes them concurrently with `tokio::task::JoinSet`.

   The flow is:

   - collect the fully materialized tool call
   - send a streaming request event when applicable
   - run each tool against `LooperTools::run_tool(...)`
   - append provider-specific tool result messages back into history
   - recursively call the same handler again

   This recursion continues until a provider response contains no more tool calls.

6. Turn completion

   `Looper` stores the returned history and exposes:

   - `steps`: every provider round-trip in the completed loop
   - `final_text`: the last step that contained assistant text
   - `message_history`: the continuation token or serialized transcript to reuse on the next turn

   `LooperStream` stores the returned history and returns it to the caller. The user-visible output is expected to come from the interface event channel, not from the `send(...)` return value.

## Contracts / invariants

- `MessageHistory` is handler-family specific. A `ResponseId` only makes sense for the OpenAI Responses handler. A serialized `Messages(...)` blob only makes sense for the provider that created it.
- Tool execution is concurrent. `LooperTools::run_tool(...)` must be safe to call from multiple tasks at the same time.
- Tool results are JSON values, not typed Rust results. Handlers pass them back to models as serialized JSON or textified JSON depending on provider requirements.
- Tool completion order is not guaranteed to match tool request order. Most handlers collect `JoinSet` results in completion order.
- `SubAgentTool` assumes the child `Looper` has the same real tool capability set as the parent, minus the sub-agent tool itself. The builders document this, but the type system does not enforce it.
- `TurnResult.final_text` is derived from the last step that has text. A tool-only turn can legitimately leave it as `None`.
- Streaming UIs should treat `ToolCallPending` as progress, not as a fully stable contract:
  - OpenAI Completions may emit pending events before the tool call ID is fully assembled.
  - Gemini creates local UUIDs for tool calls because the SDK stream does not provide a stable external ID in the same shape as other providers.
- Thinking support is provider-dependent. OpenAI Responses, Anthropic, and Gemini emit reasoning-style content. OpenAI Completions currently does not surface thinking events in this crate.

## Failure modes

- Configuring `.sub_agent(...)` without `.tools(...)` is a footgun. The system prompt is rendered as if sub-agent support exists, but no `SubAgentTool` is actually registered because the builders only inject it through a mutable tool registry.
- Using `LooperStream` without `.interface_sender(...)` means the handler still writes into its internal channel, but nothing drains it. Small turns may appear to work, but large enough output can eventually fill the channel and stall progress.
- Reusing the wrong `MessageHistory` variant with the wrong handler can fail deserialization or silently break conversation continuity.
- Tool failures are usually surfaced to the model as JSON error payloads, not as top-level Rust errors. That keeps the loop alive, but it means the model must interpret operational failures itself.
- Task join failures inside handlers are logged to stderr and the loop continues. This can hide partial tool execution failures from callers unless they inspect the resulting tool outputs carefully.
- `EmptyToolSet::add_tool(...)` panics. The builders avoid calling it directly, but any future code that mutates an `EmptyToolSet` would fail hard instead of returning a recoverable error.

# Related docs

- [`README.md`](../README.md): public positioning, quick usage examples, and a high-level sequence diagram
- [`examples/cli.rs`](../examples/cli.rs): a streamed UI integration with buffered output and a custom tool set
- [`examples/cli_non_streaming.rs`](../examples/cli_non_streaming.rs): a minimal non-streaming integration that prints `TurnResult`
- `src/mapping/tools/*`: provider-specific tool schema conversions
- `src/types/*`: the shared contracts every layer depends on
