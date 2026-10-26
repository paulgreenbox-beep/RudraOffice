[CmdletBinding()]
param([Parameter(Mandatory)][string]$AcceptanceFile,[Parameter(Mandatory)][string]$PlanFile)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& node (Join-Path $PSScriptRoot 'verification-policy.mjs') record --root $root --acceptance (Resolve-Path -LiteralPath $AcceptanceFile).Path --plan (Resolve-Path -LiteralPath $PlanFile).Path
exit $LASTEXITCODE
