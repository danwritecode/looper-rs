---
title: "Mapping Layer"
when_to_read:
  - "When you are adding a new provider and need to translate shared tool definitions into that provider's SDK types."
  - "When you are trying to understand whether `src/mapping` is part of the active runtime path or leftover normalization scaffolding."
summary: "The `mapping` module is the crate's translation boundary between internal types and provider-specific SDK types. In practice, the `tools` submodule is active runtime code used by handlers to register tools with each provider, while the `turn` submodule looks like an unfinished normalization layer because current handlers construct `TurnStep` values directly instead of using it."
ontology_relations:
  - relation: "depends_on"
    target: "src/types/tool.rs"
    note: "`LooperToolDefinition` is the common tool schema all tool mappings start from."
  - relation: "depends_on"
    target: "src/types/turn.rs"
    note: "The `turn` mappings target `TurnStep`, which is the crate's normalized non-streaming step model."
  - relation: "feeds"
    target: "src/services/handlers"
    note: "Handlers call these mappings when they need provider-native tool definitions."
  - relation: "feeds"
    target: "docs/core-agent-loop-architecture.md"
    note: "This page fills in the translation layer used by the broader agent loop."
---

# Purpose

Explain what `src/mapping` does, why it exists, and which parts of it matter to the live agent loop.

Plain-English version: this is the translation layer between the crate's internal format and each AI provider's SDK format.

# Scope

This page covers:

- `src/mapping/mod.rs`
- `src/mapping/tools/*`
- `src/mapping/turn/*`

It focuses on how internal crate types are translated into provider SDK types, and where that translation layer is currently incomplete or unused.

# Main content

## What `mapping` is

`src/mapping` is not the orchestration layer of the crate. It does not run the agent loop, execute tools, or manage conversation history.

Its job is narrower: take a crate-owned type and convert it into a provider-owned type, or vice versa.

If you want the simplest mental model, `mapping` is the translator between "our format" and "the provider's format."

There are two submodules:

- `mapping::tools`
  Converts the crate's shared tool definition into the schema each provider SDK expects.
- `mapping::turn`
  Converts provider response objects into the crate's `TurnStep` shape.

At the top level, [`src/mapping/mod.rs`](../src/mapping/mod.rs) exposes both submodules, but only re-exports `tools`.

## The central internal type

The whole `tools` mapping layer starts from [`LooperToolDefinition`](../src/types/tool.rs).

That struct is the crate's provider-agnostic tool schema:

- `name`
- `description`
- `parameters` as arbitrary JSON Schema-ish `serde_json::Value`

This is the internal intermediate representation. Tool authors build this once, and handlers then map it into provider-native tool definitions.

## Flow / behavior

## `mapping::tools`: the active part

This is the mapping code that is clearly on the runtime path today.

This is the part that most directly means "translate us to the provider." The crate defines tools in one internal format, and `mapping::tools` converts them into the exact schema OpenAI, Anthropic, or Gemini expects.

The flow looks like this:

1. A tool implementation exposes `LooperToolDefinition`.
2. A handler receives `Vec<LooperToolDefinition>` through `set_tools(...)`.
3. The handler converts those definitions into the SDK type its provider expects.
4. The provider SDK request is built using those converted tool definitions.

### OpenAI Completions

[`src/mapping/tools/openai_completions.rs`](../src/mapping/tools/openai_completions.rs) implements:

- `From<LooperToolDefinition> for ChatCompletionTool`

That is why the handler can write:

- `ChatCompletionTools::Function(t.into())`

in [`src/services/handlers/openai_completions.rs`](../src/services/handlers/openai_completions.rs).

This mapping is thin. It passes through the internal name, description, and parameter schema into OpenAI's chat-completions tool type.

### OpenAI Responses

[`src/mapping/tools/openai_responses.rs`](../src/mapping/tools/openai_responses.rs) implements:

- `From<LooperToolDefinition> for FunctionTool`

This supports the same pattern in the Responses handler:

- `Tool::Function(t.into())`

in [`src/services/handlers/openai_responses.rs`](../src/services/handlers/openai_responses.rs).

Conceptually this is the same as the completions mapping, just against a different OpenAI SDK surface.

### Anthropic

[`src/mapping/tools/anthropic.rs`](../src/mapping/tools/anthropic.rs) implements:

- `From<LooperToolDefinition> for async_anthropic::types::Tool`

The Anthropic handler uses that via plain `.into()` in [`src/services/handlers/anthropic.rs`](../src/services/handlers/anthropic.rs).

Anthropic's shape is simple here: the internal tool name becomes the external name, the description is optional, and `parameters` becomes `input_schema`.

### Gemini

Gemini is the one exception to the trait-based pattern.

[`src/mapping/tools/gemini.rs`](../src/mapping/tools/gemini.rs) exposes a function:

- `to_gemini_tool(Vec<LooperToolDefinition>) -> Tool`

instead of a `From<LooperToolDefinition>` impl.

The reason is in the code comment: the SDK's `FunctionDeclaration` does not expose the needed parameter fields publicly, so the crate constructs the Gemini schema through `serde_json::from_value(...)` instead.

The Gemini handlers call it directly in [`src/services/handlers/gemini.rs`](../src/services/handlers/gemini.rs) and the non-streaming variant.

This is the most "adapter-like" part of the mapping layer because it has to work around an SDK API limitation instead of just renaming fields.

## `mapping::turn`: a normalization layer that is not on the main path

The `turn` submodule contains provider-to-`TurnStep` conversions:

- [`src/mapping/turn/openai_completions.rs`](../src/mapping/turn/openai_completions.rs)
- [`src/mapping/turn/anthropic.rs`](../src/mapping/turn/anthropic.rs)
- [`src/mapping/turn/gemini.rs`](../src/mapping/turn/gemini.rs)

Each file implements `From<ProviderResponse> for TurnStep`.

The intent is straightforward:

- extract thinking blocks
- extract assistant text
- ignore tool calls here because handlers need to execute them and attach results

That design makes sense in theory, but there is an important practical detail: current non-streaming handlers do not call these conversions. They build `TurnStep` directly inside the handlers while they are also collecting tool call records.

So today, `mapping::turn` reads more like a partial abstraction that was started but never fully adopted.

That means the most accurate short description today is:

- active path: us -> provider, mainly for tool definitions
- partial path: provider -> us, for turn normalization, but not the main runtime path

## Why this split exists

The split between `tools` and `turn` reflects two different normalization problems:

- outbound normalization:
  turn one crate-owned tool definition into four different SDK shapes
- inbound normalization:
  turn different provider responses into one crate-owned `TurnStep`

Outbound normalization is complete enough to be useful, so it is active.
Inbound normalization is harder because tool execution is interleaved with response parsing, so the handlers currently own most of that logic themselves.

## Contracts / invariants

- `mapping::tools` assumes `LooperToolDefinition.parameters` is already valid enough for the target provider to accept.
- The OpenAI and Anthropic mappings are intentionally thin adapters. They mostly rename or reposition fields.
- The Gemini mapping is not field-by-field construction through normal SDK builders; it relies on serde to populate a type whose relevant fields are not public.
- `mapping::turn` only models text and thinking extraction. It does not model executed tool results, which is one reason handlers still need custom logic.
- The `From` impl pattern is used so handlers can stay terse and provider code can do `t.into()` instead of knowing every schema detail.

## Failure modes

- If a tool definition contains a schema the provider SDK rejects, the failure happens when the request builder or provider API consumes the mapped type, not earlier in `LooperToolDefinition`.
- Several mappings use `expect(...)` during construction. That means bad assumptions about SDK builders or schema shape can panic instead of returning a normal error.
- Gemini is the most brittle mapping because it depends on serde shape compatibility with an external SDK type rather than normal public field setters.
- `mapping::turn` can drift from real runtime behavior because it is not the code path that currently constructs `TurnStep` during actual non-streaming execution.
- The OpenAI Completions turn mapper has a visible sign of that drift: it computes `has_tool_calls` but returns `Vec::new()` in both branches, which means the conditional currently carries no behavior.

# Related docs

- [`docs/core-agent-loop-architecture.md`](./core-agent-loop-architecture.md): explains where the mapping layer sits inside the larger agent loop
- [`src/types/tool.rs`](../src/types/tool.rs): the shared tool schema all outbound mappings depend on
- [`src/types/turn.rs`](../src/types/turn.rs): the normalized non-streaming turn model the inbound mappings target
- [`src/services/handlers`](../src/services/handlers): the main consumers of the tool mappings
