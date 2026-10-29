# Writer transactional inline draft safety

Scope: source-only reliability candidate, not historical focus or native GUI acceptance.
Base: integrated Foundation main 5a71d0e221b2ab65addc941efb0ca707c97fd2e5. No Toolkit, Makepad vendor, dependency revision or lockfile change.

## Proven source defects and minimal fixes

The old UI commit removed its inline session and hid the editor before resolving its document/block and applying canonical text. Re-selecting the paragraph could also replace the draft. Changed events used active-document routing without the originating inline generation.

The new path validates document/block/revision/session epoch; preserves the last captured widget text in a token-bound draft; stages selection and replacement in the existing Editor transaction; and retires/hides only after success. Conflicts retain the editor and draft. Normal close and ruler changes are refused until an unresolved inline draft is applied or explicitly cancelled. The ruler guard runs before canonical or layout mutation and is covered by a real RED/GREEN state test. Late widget actions carry their issuing session metadata and cannot update a reopened session or another active document.

Core commit/Undo/Redo now detect revision exhaustion before publishing document, selection or history. Transaction::execute stages on a copy so a failed command leaves the prior staged state intact. WriterViewModel refreshes its existing cache and clears stale search results only after successful replacement. This is not a new document model, engine or runtime.

## Verification scope

Actual local owning results before publication: writer-core 24, writer-api 19, writer-ui 16 passed; app 273 passed and 2 explicit private-operator tests ignored. Strict owning/app Clippy and first-party rustfmt passed. Initial core RED was 1 pass/5 failures; GREEN 8/8 followed an actual rebuild. A stale-binary retry and initial production-cfg Clippy failure were retained separately, not labelled final passes. The ruler regression separately failed on the previous candidate, then passed after the three-line guard; old candidate CI is not treated as verification of that added guard.

Coverage: deleted target, guarded close plus registry-removed document, tab switch, revision conflict, stale epoch, Cyrillic/emoji scalar offsets, retry, one-operation Undo/Redo and retire-once. Real canonical-error atomicity is tested at core and view-model boundaries; app draft/session retention is tested on validation/deleted/revision failures. There is no direct backend-error injection through the AppState boundary. Clipboard isolation is an application unit test using synthetic ClipboardObservation, not the OS clipboard.

The public Windows workflow additionally runs the complete writer-core and writer-ui targets so new production-path regressions are not merely dependency compilations. Source-bound CI results must be recorded independently, not inferred from these local counts.

CHEAPEST_VERIFICATION_PATH: focused core/API/UI → owning Writer/app → strict Clippy/fmt → required approved native GUI and both selected expanded L4b compositions → release-only frozen smoke. The first three are source checks, not permission to skip the remaining selected gates. Use verification/inputs/RDO-012-writer-transactional-inline-commit.json with the existing planner. No acceptance-history or cadence counter is advanced here.

## Deliberate limitations

Historical focus root cause remains UNDETERMINED. The Draw guard prevents requesting focus for a different inline document; it is not an actual KeyFocus acknowledgement or proof of the historical intermittent cause. Native focus and clipboard checks remain NOT_RUN/BLOCKED_NOT_EXPOSED.

Widget-session metadata is not a Toolkit UIA consume-once ticket. Public checked-action integration remains blocked until a reviewed owner API exists. Existing preview/RC binaries and the original development checkout are not silently updated. This candidate is not a production release or full Office fidelity claim.
