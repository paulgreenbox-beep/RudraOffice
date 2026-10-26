# Toolkit shared-core migration — 2026-10-10

The authoritative manifest and all recipes/history below remain consumer-owned.
The existing PowerShell entrypoints now resolve Toolkit's shared core 1.0.0 via
`verification/toolkit-core.json`: relative checkout (or trusted operator
`RUST_MAKEPAD_TOOLKIT_ROOT`), package version and exact module hashes. A dependency
upgrade requires explicit re-pinning; a floating branch is not accepted.

Configured CLI plans/record use evidence v2. Legacy library exports and isolated
unconfigured v1 fixtures remain compatible; they do not confer new v2 E2E proof.
Existing RC28 plans/reports are preserved, NOT_RUN/PARTIAL, not silently upgraded.
Generate a new attempt plan before execution. `verification-run-envelope.mjs
finish` additionally requires `--observations <fresh-observer-sidecar.json>`.
The existing executor must supply actual action/readback correlation, runtime
identity and cleanup telemetry; this core never launches GUI or invents sidecars.
Missing v2 observations fail closed. No Office milestone has been accepted by
this migration, no production code or EXE was changed.

For current schema, CLI, resources and proof fields, read the resolved Toolkit's
`docs/VERIFICATION_ECONOMY.md`. Below is the preserved original consumer guide;
its historical evidence-v1 examples are not an exemption from v2 safeguards.

---

# Verification economy

Source of truth: `verification/verification-policy.json` (schema 1). This guide
explains it; it does not hold a second counter. This is verification infrastructure,
not a change to Office functionality. The existing Toolkit plugin/skills route
agents to this manifest; no second verification plugin is installed.

## Levels

| Level | Proof |
|---|---|
| L0 | Pure/unit algorithms, parsing, limits, transactions and state |
| L1 | Owning crate/component behavior |
| L2 | App commands, typed session/history isolation and worker routing |
| L3 | Real focused pointer, keyboard, focus, native clipboard, rendering or UIA |
| L4a | Four small golden workflows |
| L4b | Periodic/risk cross-feature composition, not every historical script |
| L5 | One short frozen release artifact launch/critical path/health smoke |

These consumer acceptance labels are not the Toolkit's generic Cargo L0–L4.
Prove each invariant at the cheapest sufficient level. Do not delete existing
tests, reduce assertions or avoid GUI proof of an actual new interaction.
Process launches, document opens, native actions, captures, clipboard ownership
and exclusive desktop time dominate cost; assertion count does not.

## Cadence and triggers

Default interval: five **accepted functional milestones**. Planning, failed or
PARTIAL candidates, docs, infrastructure and retries do not count. A verified
L4b within functional acceptance resets the counter to zero even when triggered
early by risk. Counter and append-only history are updated atomically in one JSON.

Current imported legacy baseline is UNKNOWN: RC25–27 have historical acceptance,
but no recorded composition identity under this policy. Do not invent a reset
from assertion totals. RC28 remains PARTIAL and is not counted. Its risk-triggered
closure can establish the first confirmed baseline after actual acceptance.

L4b is immediate for shared shell/tabs/runtime, dispatcher/global history,
keyboard/pointer/Makepad routing, Win32/native clipboard, workers, viewport/layout/
rendering, UIA, save, shared Toolkit/vendor behavior, real-user regression,
unexplained GUI flakes, broken accepted invariants or significant external baseline.
Mandatory expanded coverage is never silently removed to fit a budget.

## Planning and validation

From the explicit Rudra Office root:

```powershell
node --test verification/tests/verification-policy.test.mjs
./tools/validate-verification-policy.ps1
./tools/plan-verification.ps1 -Milestone NEXT -ChangedModules sheets-formula-helper
./tools/plan-verification.ps1 -InputFile verification/inputs/RC28-closure.json
```

The last command writes `verification/current-verification-plan.json`.
Milestone-local output is `verification/plans/RC28-closure.json`.
Node CLI: `tools/verification-policy.mjs validate|plan|record`. It plans and
checks evidence; it does not run Cargo, GUI, the EXE or clipboard operations.

Input: milestone, kind, changed files/modules, feature tags, optional risk and
budget reason, optional release candidate/artifact identity. Map unrecognized
paths before execution. Output records required L0–L5, selected and skipped
scenarios with reasons, risks, unchanged counter, costs and soft budget status.
`planning_input` and policy digest bind the plan; edited/dropped coverage cannot
be used for acceptance. `NEEDS_IMPACT_REVIEW`, `NEEDS_FIXTURE_REVIEW`,
`BLOCKED_TEST_INVESTIGATION` and `BUDGET_JUSTIFICATION_REQUIRED` are not runnable
acceptance plans.

Soft budgets: focused 4, routine 4, representative real files 2, smoke 1, typical
total 6–10. A justified overflow records `budget_override_reason`, risk triggers
and required scenario IDs. It does not truncate required tests.

## Recipes and RC28

The 16 registered recipes use the existing public AI UI Test Tool Script API.
Generate only recipes with `node tools/materialize-verification-scenarios.mjs`,
then validate with a hash/provenance-verified public runner's `validate` command.
Validation is NOT execution or GUI acceptance. Recipes are pinned to RC28;
before a future artifact, deliberately regenerate/review command identity.

Four goldens: selection/mutation/Undo/Redo; sort/filter/navigation; structure/
formula/Undo; two-book isolation/switch/close. The expanded Sheets scenario
composes those workflows with clipboard in one process. The expanded Office
scenario exercises cross-format session activation/close in one process.
Their declared subsumption avoids rerunning identical standalone goldens in
the same closure, but only a complete new composition PASS proves them.

RC28 closure: five focused launches (four clipboard boundaries + grid/resize),
one original XLSX Copy-only, two compositions, one final smoke = **9**, versus
24 feature/historical launches + smoke = 25 in the historical plan. The fifth
focused launch is explicitly justified for rendering/selection changed alongside
Win32 input. Native/system clipboard, rich/cross-book, deferred Cut, focus
(cell/search/address), Windows sequence ownership and external TSV remain.
The oversized native case requires a controlled X-pattern OS seed of 1,048,577
UTF-8 bytes on a free desktop using the already-existing Windows clipboard
fixture, not a invented Script API clipboard operation. Preserve/restore the
user's clipboard through the existing harness and record interference distinctly.

Original XLSX is read/Copy-only; never mutate/save it. Check its manifest hash
before and after the future GUI run. Recipe validation, screenshots or historical
RC25–27 reports are not fresh RC28 PASS. New recipes are currently NOT_RUN.
The PPTX parser recipe is deliberately held for representative fixture review;
a blank slideshow proves lifecycle only, not a real PPTX parser.

## Lifecycle and outcomes

Registry states: NEW_CRITICAL, ROUTINE_GUI, PERIODIC_GUI, RISK_TRIGGERED,
LOWER_LEVEL_ONLY, RETIRED. Demote only with known root cause, trustworthy L0–L2
coverage and multiple stable subsequent GUI runs recorded by the owner. No
automatic demotion or deletion of historical evidence. Promote after a real
GUI defect, an uncovered integration boundary or recurrence.

Stability: stable, flaky_candidate, quarantined. Same frozen bytes/preconditions
but differing outcomes with unexplained cause require investigation ownership.
Quarantine is not PASS. Periodic and risk composition remains mandatory.

PASS needs verified postconditions and healthy runtime. Foreground loss,
external clipboard replacement and unavailable desktop are BLOCKED_ENVIRONMENT.
Proven stale UIA, DPI mismatch and invalid script are FAIL_TEST_INFRA. Actual
product defects are FAIL_PRODUCT. Unexecuted/insufficient proof is NOT_RUN.
Retry only blocked scenarios on unchanged bytes/source/preconditions. After
production changes repeat failing + directly impacted + required final closure;
do not restart all history without a recorded risk reason.

## Acceptance state update

After real acceptance, supply an event to:

```powershell
./tools/update-verification-state.ps1 -AcceptanceFile verification/acceptance.json -PlanFile verification/current-verification-plan.json
```

Event fields: milestone, status=ACCEPTED, kind=functional, date, artifact_hash,
project-relative artifact_path, quality_report, source_freeze, expanded_gui and
scenario_results (`id`, PASS, artifact_hash, report, run_manifest). Quality JSON
contains artifact.sha256, completed_at, source_freeze_sha256 and gates with IDs
from the selected L0–L2 tests and exit_code=0.
Source freeze is a nonempty list of path/sha256 pairs with actual unchanged files.
Use `tools/verification-run-envelope.mjs begin --plan <plan> --scenario <id>
--source-freeze <freeze> --output verification/runs/<unique-id>.json` before
the existing public harness, then `finish --run <envelope> --report <raw-report>`
afterward. The tool only captures pre/post artifact/source/recipe/report hashes
and timestamps; it never launches a runner, changes clipboard or marks PASS.
Keep each attempt's envelope and raw report. Do not overwrite old evidence.

Each selected GUI result needs its own complete public raw report, matching
recipe name, all checks passed/non-skipped, live healthy process and explicitly
clean stderr and every recipe setup/step/teardown index/op. The run envelope
must match the actual frozen bytes, source, recipe and raw-report hash. L5 must
follow quality and all required GUI and is required exactly once when
release_candidate=true. A functional event cannot use an infrastructure plan.
Partial/docs/infra events return unchanged. Repeated accepted identities are rejected.

These checks are provenance/consistency gates for a trusted execution workflow,
not protection against a human fabricating evidence files. Actual OS clipboard,
screenshots and causal runtime evidence still belong to the public harness.
Instructions are not an OS-level command interceptor. No automatic acceptance
or background GUI is installed.
