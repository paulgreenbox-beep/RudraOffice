# GUI development route — 2026-10-26

The project route is `toolkit-gui-remote-canary`, defined in
`verification/gui-development.json`. Validate it with
`node tools/verify-gui-development.mjs` and
`node --test tools/gui-development.test.mjs`.

## Current capability boundary

The connected canary exposes exactly five read-only operations: control tree,
element state, AutomationId state, window capture, and its evidence journal.
It does not expose window discovery, application launch/close, Invoke, text,
keyboard, pointer or clipboard operations. Do not infer those capabilities
from a larger historical server catalog or a previous input regression.

A target window ID must come from this canary's owning session/approved target
binding. Do not copy a `window_id` from the legacy GUI connection, manufacture
an opaque identifier from HWND/PID, or start an unmanaged App to bypass this
binding. If no approved target is available, tree/capture proof is NOT_RUN.

No automatic legacy fallback is allowed. An unavailable operation remains
BLOCKED_NOT_EXPOSED. In particular, an evidence-journal call cannot establish
Writer editing, Slides hit testing, PDF navigation or Sheets Copy/Cut/Paste.
Those gates still require real actions, independent document-state readbacks,
artifact/source identity, health and owned cleanup under the project's policy.

## Libraries are not the MCP connection

The existing Makepad/Toolkit Cargo libraries, typed registry, page/canvas
primitives and verification Core remain dependencies. This workflow change is
not a Makepad SDK upgrade, a new runner, a new MCP server or a global Codex
configuration change. Historical recipes and immutable reports stay intact;
they are not replayed or promoted by selecting this connection.

Current Toolkit Core 1.0.0 imports `executable-provenance.mjs`; the upstream thin
consumer adapter and all seven module hashes are pinned in this project.
Normal Windows PE workflows remain on the existing `makepad-windows` profile.
No ModuleShell/WSLC RAM waiver or global policy override is applied.

The next mutable GUI gate requires the Toolkit owner to provide a qualified,
approved Rudra target/session and the necessary exposed operations. It is not
enabled by editing this project's capability declaration.
