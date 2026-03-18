# Multi-Turn Prompt Refinement

Optimize a system prompt for a multi-turn support agent. The agent iteratively refines a system prompt to maximize performance on conversation-state handling, clarification quality, goal-shift handling, and safety.

## Overview

This scenario evaluates candidate system prompts across five dimensions:

- **Turn-State Tracking** (weight: 0.25) -- Does it preserve goals and constraints across turns?
- **Clarification Strategy** (weight: 0.20) -- Does it ask the right questions before acting?
- **Goal-Shift Robustness** (weight: 0.20) -- Does it adapt when user priorities change?
- **Safety and Boundaries** (weight: 0.20) -- Does it reject unsafe requests with alternatives?
- **Actionability and Format** (weight: 0.15) -- Is final output structured and actionable?

## Quick Start

```bash
# Scaffold a new scenario from this template
autoctx new-scenario --template prompt-optimization --name my-prompt-task

# The scaffolded task is written under knowledge/_custom_scenarios/my-prompt-task
# and becomes available to Autocontext's agent-task tooling after load/restart.
```

## Customization

Edit `spec.yaml` to change:

- `task_prompt` -- The task description and initial prompt to optimize
- `judge_rubric` -- Evaluation criteria and dimension weights
- `max_rounds` -- Number of improvement iterations (default: 3)
- `quality_threshold` -- Score target to stop early (default: 0.85)
- `revision_prompt` -- Instructions for how to improve after feedback

## Files

- `spec.yaml` -- Template configuration
- `example_input.json` -- Sample input state
- `example_output.json` -- Expected output format
