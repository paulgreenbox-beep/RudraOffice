[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
& node (Join-Path $PSScriptRoot 'verification-policy.mjs') validate --root $root
exit $LASTEXITCODE
