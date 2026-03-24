---
title: "Tools Layer"
when_to_read:
  - "When you are implementing a new tool or tool registry and need to understand what the handlers expect."
  - "When you are debugging tool execution, sub-agent injection, or why a model sees a tool but cannot use it correctly."
summary: "The `tools` module is the capability boundary between the model-facing agent loop and your application logic. A `LooperTool` describes and executes one tool, while a `LooperTools` implementation is the registry and dispatcher that exposes tools to handlers, runs them by name, and optionally accepts injected capabilities like the built-in sub-agent tool."
ontology_relations:
  - relation: "depends_on"
    target: "src/types/tool.rs"
    note: "The tools layer uses `LooperToolDefinition` as the provider-agnostic schema for advertising tools."
  - relation: "feeds"
    target: "src/services"
    note: "Handlers call `LooperTools::get_tools` and `LooperTools::run_tool` to register and execute model-requested tools."
  - relation: "feeds"
    target: "src/mapping/tools"
    note: "Tool definitions from this layer are translated into provider-native tool schemas there."
  - relation: "feeds"
    target: "docs/core-agent-loop-architecture.md"
    note: "This page explains the capability system used by the broader agent loop."
---

# Purpose

Explain what `src/tools` does and how it fits into the runtime.

Plain-English version: this is the layer where the model's requested actions become your app's real code.

# Scope

This page covers:

- `src/tools/mod.rs`
- `src/tools/empty.rs`
- `src/tools/sub_agent.rs`
- the example `ToolSet` implementations in `examples/cli.rs` and `examples/cli_non_streaming.rs`

It focuses on the tool contract, the registry contract, and the built-in behavior this crate provides out of the box.

# Main content

## What `tools` is

The `tools` layer is the capability boundary of the crate.

It answers two different questions:

- what tools exist and how should they be described to the model?
- when the model asks for a tool by name, how do we actually run it?

That is why the module has two traits instead of one.

## Structure

[`src/tools/mod.rs`](../src/tools/mod.rs) defines:

- `LooperTool`
  one executable tool
- `LooperTools`
  a tool registry plus dispatcher

The built-ins are:

- [`EmptyToolSet`](../src/tools/empty.rs)
  a no-tools fallback
- [`SubAgentTool`](../src/tools/sub_agent.rs)
  a special tool that wraps another `Looper`

## The two levels of abstraction

### `LooperTool`: one tool

[`LooperTool`](../src/tools/mod.rs) has three responsibilities:

- `tool() -> LooperToolDefinition`
  returns the schema the model sees
- `get_tool_name() -> String`
  returns the dispatch key used by the registry
- `execute(&mut self, args: &Value) -> Value`
  performs the real work and returns a JSON result

This means a single tool has both:

- a declarative side
  name, description, parameters
- an operational side
  actual execution against runtime input

The args and result are both `serde_json::Value`, so the tool boundary is intentionally dynamic rather than strongly typed.

### `LooperTools`: the registry

[`LooperTools`](../src/tools/mod.rs) is the surface the rest of the runtime calls.

It has three responsibilities:

- `get_tools()`
  enumerate tool definitions for provider registration
- `add_tool(...)`
  mutate the registry by adding a tool
- `run_tool(name, args)`
  dispatch a model-requested tool call by name

Plain-English version:

- `LooperTool` is one tool
- `LooperTools` is the toolbox

## Flow / behavior

1. Tool authoring

   A tool implementation returns a [`LooperToolDefinition`](../src/types/tool.rs) from `tool()`.

   That definition includes:

   - name
   - description
   - parameter schema

2. Registry exposure

   A `LooperTools` implementation collects all tool definitions and returns them from `get_tools()`.

   This is what `Looper` and `LooperStream` pass into handlers during build.

3. Provider registration

   The handlers convert those generic tool definitions into provider-native schemas through the mapping layer.

4. Runtime execution

   When a provider response contains a tool call, the handler invokes:

   - `run_tool(name, args)`

   on the registry.

   The registry finds the matching `LooperTool` and calls `execute(...)`.

5. Result return

   The JSON result goes back to the handler, which packages it into the provider-specific tool-result message format and continues the loop.

So the tools layer is where the abstract model request becomes concrete application behavior.

## The example registry pattern

The example CLIs show the intended usage pattern well.

In [`examples/cli_non_streaming.rs`](../examples/cli_non_streaming.rs) and [`examples/cli.rs`](../examples/cli.rs), the registry:

- stores tools in a `HashMap<String, Mutex<Arc<dyn LooperTool>>>`
- exposes schemas by iterating over tools and calling `tool()`
- dispatches by tool name in `run_tool(...)`

This reveals an important design choice in the crate:

- the library defines the tool contract
- the caller owns the actual registry implementation

There is no full built-in mutable tool registry in `src/tools`; you are expected to provide one.

## Built-in tool implementations

### `EmptyToolSet`

[`EmptyToolSet`](../src/tools/empty.rs) is the default fallback when no tools are provided.

Its behavior is intentional:

- `get_tools()` returns an empty list
- `run_tool(...)` returns a JSON error saying the function is unknown
- `add_tool(...)` panics

This makes it safe as a read-only "no tools available" fallback, but unsafe as a mutable registry.

### `SubAgentTool`

[`SubAgentTool`](../src/tools/sub_agent.rs) is a special built-in tool that wraps another non-streaming `Looper`.

Its schema exposes one argument:

- `task_description: string`

Its execution path is:

1. read `task_description`
2. call the child `Looper`
3. return either:
   - `{ "agent_findings": ... }`
   - or an error payload

This lets the parent model delegate work to a child agent without knowing anything about the child implementation beyond the tool interface.

## How sub-agent injection really works

The builders in `Looper` and `LooperStream` can inject `SubAgentTool` automatically when `.sub_agent(...)` is configured.

That only works if the caller supplied a real mutable `LooperTools` implementation. The builders inject the sub-agent by calling `add_tool(...)` on that registry before registering tool definitions with the handler.

So the tools layer is not just a passive trait boundary. It is also the place where the runtime mutates available capabilities during build.

## Contracts / invariants

- `tool().name` and `get_tool_name()` must effectively agree. If they diverge, the model may request one name while the registry dispatches under another.
- Tool args and results are JSON values. Validation is the tool author's job, not the framework's.
- `run_tool(...)` must be safe under concurrent access because handlers execute tool calls in parallel.
- A `LooperTools` implementation must be able to expose tool schemas and execute the same tools consistently by name.
- If you want sub-agent support, your registry must support `add_tool(...)`.
- Tools are allowed to be stateful because `execute(...)` takes `&mut self`, but the registry is responsible for making mutable access safe.

## Failure modes

- Unknown tool names do not usually cause a top-level Rust error. They typically come back as JSON error payloads from the registry.
- `EmptyToolSet::add_tool(...)` panics, so it cannot be used as a mutable registry.
- If `tool().name` and `get_tool_name()` do not match, the model-facing schema and runtime dispatch key can diverge in subtle ways.
- A registry that is not concurrency-safe can corrupt state or deadlock when handlers run multiple tools at once.
- The example registry pattern uses `Arc::get_mut(...)`, which assumes the registry holds the only strong reference to each tool. If that assumption stops being true, execution will panic.
- `SubAgentTool` depends on a child `Looper` that is expected to have the same practical tool capability set as the parent, minus sub-agent recursion. That assumption is documented but not enforced by the type system.

# Related docs

- [`docs/services-layer.md`](./services-layer.md): explains how handlers consume tool registries and execute tool calls
- [`docs/mapping-layer.md`](./mapping-layer.md): explains how tool definitions are translated into provider-native schemas
- [`src/types/tool.rs`](../src/types/tool.rs): the shared tool definition type used by every tool
- [`src/looper.rs`](../src/looper.rs): shows where builders inject `SubAgentTool` into a mutable registry
- [`src/looper_stream.rs`](../src/looper_stream.rs): the streaming builder follows the same injection pattern
