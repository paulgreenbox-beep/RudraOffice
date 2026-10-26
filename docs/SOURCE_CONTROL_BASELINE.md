# Initial Public Source Baseline

Public source snapshot prepared in an isolated publication directory. The original development tree, nested Slides repository, working changes, private documents, evidence and frozen executables are not moved or rewritten.

Publication verification and the initial commit identity will be recorded in reports/github-publication-2026-10-27.json after the first real public push. A local commit alone is not publication success.

Development workflow after publication:

1. Keep main as the integrated source baseline.
2. Use focused feature branches for Writer, Sheets, Slides or PDF as work is needed; do not create empty branches in advance.
3. Review changes through Pull Requests with integration/verification ownership.
4. Preserve upstream attribution and record changes to vendored engines.
5. Keep private fixtures, credentials, capsules, logs and executable candidates out of commits.
6. Promote a release/tag only after separate release acceptance. No production executable is released by this baseline.

Local and browser development/RC acceptance histories are not imported as Git commits or rewritten into a production-readiness claim.
