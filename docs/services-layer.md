---
title: "Services Layer"
when_to_read:
  - "When you are adding or changing a provider handler and need to understand the runtime contract `Looper` depends on."
  - "When you are debugging how a turn becomes provider requests, tool execution, streamed events, and updated message history."
summary: "The `services` module is the provider execution layer of the crate. `Looper` and `LooperStream` select a handler from this module, then the handler owns the real work: restore history, send the request, parse text and reasoning, execute tools, recurse until no more tool calls remain, and return either a `TurnResult` or updated `MessageHistory`."
ontology_relations:
  - relation: "depends_on"
    target: "src/types"
    note: "The handler contracts depend on shared types like `MessageHistory`, `TurnResult`, and tool definitions."
  - relation: "depends_on"
    target: "src/tools/mod.rs"
    note: "Handlers execute tool calls through `LooperTools`."
  - relation: "depends_on"
    target: "src/mapping/tools"
    note: "Some handlers use tool mappings to convert internal tool definitions into provider SDK types."
  - relation: "feeds"
    target: "src/looper.rs"
    note: "`Looper` delegates non-streaming turn execution to this layer."
  - relation: "feeds"
    target: "src/looper_stream.rs"
    note: "`LooperStream` delegates streaming turn execution to this layer."
---

# Purpose

Explain what `src/services` does and why it is the main runtime layer of the crate.

Plain-English version: if `Looper` is the shell, `services` is the part that actually talks to the model providers and runs the agent loop.

# Scope

This page covers:

- `src/services/mod.rs`
- `src/services/chat_handler.rs`
- `src/services/chat_handler_streaming.rs`
- `src/services/handlers/*`

It focuses on the contract the rest of the crate relies on and the common execution pattern shared across provider handlers.

# Main content

## What `services` is

`services` is the provider driver layer.

It is the code that knows how to:

- talk to OpenAI, Anthropic, or Gemini
- restore provider-specific conversation history
- register provider-native tool definitions
- parse model output into text, reasoning, and tool calls
- run tools through the shared `LooperTools` interface
- keep looping until the provider stops asking for tools

This is where most of the real agent behavior lives.

## Structure

There are three parts:

- [`src/services/chat_handler.rs`](../src/services/chat_handler.rs)
  Defines the non-streaming trait `ChatHandler`.
- [`src/services/chat_handler_streaming.rs`](../src/services/chat_handler_streaming.rs)
  Defines the streaming trait `StreamingChatHandler`.
- [`src/services/handlers/mod.rs`](../src/services/handlers/mod.rs)
  Exposes one concrete handler per provider and mode.

The concrete handlers are:

- OpenAI Completions streaming and non-streaming
- OpenAI Responses streaming and non-streaming
- Anthropic streaming and non-streaming
- Gemini streaming and non-streaming

## The core contracts

### Non-streaming

[`ChatHandler`](../src/services/chat_handler.rs) defines:

- `send_message(...) -> Result<TurnResult>`
- `set_tools(Vec<LooperToolDefinition>)`

This contract means the handler must finish the whole agent loop before returning. It is responsible for constructing the final `TurnResult`, including intermediate `TurnStep` values and the next `MessageHistory`.

### Streaming

[`StreamingChatHandler`](../src/services/chat_handler_streaming.rs) defines:

- `send_message(...) -> Result<MessageHistory>`
- `set_tools(Vec<LooperToolDefinition>)`

This contract means the handler sends progress out incrementally and only returns the new conversation continuation state. The visible output is emitted through `HandlerToLooperMessage` events and then forwarded by `LooperStream`.

## Flow / behavior

Even though the provider SDK code differs, the handlers mostly follow the same loop shape.

1. Restore history

   The handler accepts an optional `MessageHistory` and restores provider-specific state before adding the new user message.

   There are two history models:

   - `MessageHistory::Messages`
     used by OpenAI Completions, Anthropic, and Gemini
   - `MessageHistory::ResponseId`
     used by OpenAI Responses

2. Build the provider request

   The handler takes:

   - the current provider-specific history
   - the user message or recursive tool-result input
   - the currently registered tool definitions
   - provider-specific options like thinking or reasoning configuration

   and turns that into one provider API request.

3. Parse the provider response

   The handler extracts some combination of:

   - assistant text
   - reasoning or thinking content
   - tool call requests

   In non-streaming mode, this gets accumulated into a `TurnStep`.
   In streaming mode, this gets emitted as `HandlerToLooperMessage` values while the response is arriving.

4. Execute tools concurrently

   When tool calls are present, handlers run them through `Arc<dyn LooperTools>` using `tokio::task::JoinSet`.

   This is the agentic part of the services layer:

   - the model asks for tool calls
   - the handler executes them
   - the handler converts results into provider-native tool-result messages
   - the handler calls itself again

5. Recurse until done

   Most handlers use an `inner_send_message(...)` helper marked with `#[async_recursion]`.

   That helper keeps calling itself until the provider returns a response with no more tool calls. At that point:

   - non-streaming handlers finalize `TurnResult`
   - streaming handlers emit `TurnComplete` and return updated `MessageHistory`

## A concrete example

[`OpenAIResponsesNonStreamingHandler`](../src/services/handlers/openai_responses_non_streaming.rs) shows the pattern clearly:

- it restores `previous_response_id`
- builds a Responses API request
- extracts reasoning, text, and function calls from `response.output`
- runs function calls concurrently
- pushes `FunctionCallOutput` items back into the next recursive request
- appends a `TurnStep` for each provider round-trip

Anthropic and Gemini do the same job with different SDK shapes:

- Anthropic stores full assistant content blocks, then appends `ToolResult` user messages
- Gemini stores `Part` lists and appends `FunctionResponse` parts

## What changes across providers

The high-level loop is stable, but the provider mechanics are not.

- OpenAI Responses is the most server-state-oriented handler.
  It continues the conversation through `previous_response_id` instead of a locally stored message vector.
- OpenAI Completions stores a full local message list and appends assistant and tool messages directly.
- Anthropic models thinking, text, and tool-use as content blocks inside assistant messages.
- Gemini models text and tool calls as `Part` values and needs a small mapping helper to build tool schemas.

So `services` is not one abstract engine with tiny provider plugins. It is a shared control pattern implemented separately for each provider family.

## Contracts / invariants

- A handler must accept shared tool definitions through `set_tools(...)`, but it is free to store them in any provider-native shape internally.
- The `tools_runner` argument is the execution boundary. Handlers do not know how tools work; they only invoke `run_tool(...)`.
- The services layer owns provider-specific history restoration. `Looper` only stores and passes `MessageHistory`; it does not interpret it.
- Non-streaming handlers must reconstruct `TurnStep` and `TurnResult` themselves. This is why much of the runtime logic lives here rather than in `mapping::turn`.
- Streaming handlers must emit events in a sequence the UI can interpret:
  - assistant text deltas
  - optional thinking content
  - tool progress and completion
  - final turn completion
- Tool calls are executed concurrently, so result order can differ from request order.

## Failure modes

- Passing the wrong `MessageHistory` variant to a handler can break deserialization or conversation continuity.
- Several handlers swallow provider stream errors by printing them instead of returning them immediately. That can make failures look like partial or odd output instead of a hard error.
- Tool execution join errors are logged and the loop continues, which means some tool failures can be degraded into incomplete model context rather than surfaced as a top-level failure.
- Provider setup failures happen here, not in `Looper`:
  - missing Gemini API environment variables fail during handler construction
  - provider request-building errors fail inside the handler before or during the API call
- Because recursion is owned by the handlers, any bug in provider-specific tool-result encoding can trap a turn in a broken follow-up cycle or cause the model to lose context after a tool call.

# Related docs

- [`docs/core-agent-loop-architecture.md`](./core-agent-loop-architecture.md): broader architecture across `Looper`, handlers, tools, and history
- [`docs/mapping-layer.md`](./mapping-layer.md): the translation layer some handlers rely on for provider-native tool schemas
- [`src/looper.rs`](../src/looper.rs): the non-streaming entry point that selects a `ChatHandler`
- [`src/looper_stream.rs`](../src/looper_stream.rs): the streaming entry point that selects a `StreamingChatHandler`
