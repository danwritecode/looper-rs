# Prompt Refinement Report (Outline)

## 1) Goal

- Evaluate whether prompt refinements improve multi-turn agent behavior.
- Compare a baseline prompt against stronger protocol-style prompts.
- Use repeatable batches (10 runs per prompt) and avoid one-off conclusions.

## 2) Prompt Variants Evaluated

### Baseline

- `multi_turn_prompt_refine`
  - File: `knowledge/_custom_scenarios/multi_turn_prompt_refine/agent_task.py`
  - Seed style: generic assistant prompt adapted for multi-turn support.

### Alternative Prompt Families

- `multi_turn_prompt_refine_state_ledger`
  - File: `knowledge/_custom_scenarios/multi_turn_prompt_refine_state_ledger/agent_task.py`
  - Style: state ledger per turn.
- `multi_turn_prompt_refine_clarify_plan`
  - File: `knowledge/_custom_scenarios/multi_turn_prompt_refine_clarify_plan/agent_task.py`
  - Style: Clarify -> Plan -> Execute -> Verify.
- `multi_turn_prompt_refine_contract_protocol`
  - File: `knowledge/_custom_scenarios/multi_turn_prompt_refine_contract_protocol/agent_task.py`
  - Style: contract-first response block.
- `multi_turn_prompt_refine_state_machine`
  - File: `knowledge/_custom_scenarios/multi_turn_prompt_refine_state_machine/agent_task.py`
  - Style: phased state-machine control loop.
- `multi_turn_prompt_refine_protocol_hardgate`
  - File: `knowledge/_custom_scenarios/multi_turn_prompt_refine_protocol_hardgate/agent_task.py`
  - Style: hard-gated incident-command protocol.
- `multi_turn_prompt_refine_state_machine_longform`
  - File: `knowledge/_custom_scenarios/multi_turn_prompt_refine_state_machine_longform/agent_task.py`
  - Style: long-form state machine with explicit gates and response contract.

## 3) Testing Harness Evolution

### Phase A: Initial Judge-Only Scoring

- Original `evaluate_output()` used `LLMJudge` to score prompt text directly.
- Outcome pattern: many variants clustered near `0.90`, weak separation.
- Conclusion: this over-measured prompt wording quality and under-measured real behavior.

### Phase B: Behavioral Multi-Turn Harness (Current)

- Updated `evaluate_output()` for:
  - `knowledge/_custom_scenarios/multi_turn_prompt_refine/agent_task.py`
  - `knowledge/_custom_scenarios/multi_turn_prompt_refine_state_machine_longform/agent_task.py`
- Harness now simulates assistant replies on four concrete probes:
  1. Missing details (should ask high-impact clarifying questions)
  2. Goal shift (should acknowledge and update plan)
  3. Policy boundary request (should refuse + offer safe alternatives)
  4. Conflicting constraints (should detect conflict and request precedence)
- Output format also checked for required sections:
  - `Decision`, `Rationale`, `Next Steps`, `Open Questions`
- Weighted score (0.0-1.0):
  - `turn_state_tracking`: 0.25
  - `clarification_strategy`: 0.20
  - `goal_shift_robustness`: 0.20
  - `safety_boundaries`: 0.20
  - `actionability_format`: 0.15

## 4) Batch Protocol

- No ad-hoc single-run claims.
- Standard batch: 10 runs per prompt.
- Provider setup used for runs:
  - `AUTOCONTEXT_AGENT_PROVIDER=openai`
  - `AUTOCONTEXT_JUDGE_PROVIDER=openai`
  - `AUTOCONTEXT_MODEL_COMPETITOR=gpt-4o-mini`
  - `AUTOCONTEXT_AGENT_DEFAULT_MODEL=gpt-4o-mini`
  - `AUTOCONTEXT_JUDGE_MODEL=gpt-4o-mini`

## 5) Main Outcome We Observed (Behavioral Harness)

### A/B Comparison: Baseline vs Actual Multi-Turn Prompt

- Baseline scenario: `multi_turn_prompt_refine`
  - Run IDs: `looper-multiturn-behavior-base-batch1-r01..r10`
  - Min/Max/Avg: `0.6767 / 0.83 / 0.69653`
  - `> 0.90`: `0/10`

- Actual scenario: `multi_turn_prompt_refine_state_machine_longform`
  - Run IDs: `looper-multiturn-behavior-actual-batch1-r01..r10`
  - Min/Max/Avg: `0.77 / 0.98 / 0.87875`
  - `> 0.90`: `5/10`

### Net Effect

- Average lift (`actual - baseline`): `+0.18222`
- Behavioral harness shows clear separation in favor of the long-form protocol prompt.

## 6) Interpretation

- Prompt improvements are now measurable against task-like behavior, not just judge impressions.
- The long-form protocol prompt materially outperforms baseline on multi-turn safety/clarification/goal-shift behavior.
- Remaining variance indicates more tightening is still possible.
