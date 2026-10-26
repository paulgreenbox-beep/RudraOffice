[CmdletBinding()]
param(
 [string]$Milestone,
 [string[]]$ChangedFiles=@(),
 [string[]]$ChangedModules=@(),
 [string[]]$FeatureTags=@(),
 [string[]]$RiskOverride=@(),
 [ValidateSet('functional','infrastructure','documentation')][string]$Kind='functional',
 [switch]$ReleaseCandidate,
 [string]$InputFile='',
 [string]$OutputFile='',
 [string]$BudgetOverrideReason=''
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$arguments=@((Join-Path $PSScriptRoot 'verification-policy.mjs'),'plan','--root',$root)
if($InputFile){$arguments+=@('--input',(Resolve-Path -LiteralPath $InputFile).Path)}
else {
 if(-not $Milestone){throw 'Specify Milestone or InputFile'}
 $arguments+=@('--milestone',$Milestone,'--kind',$Kind,'--release',$ReleaseCandidate.IsPresent.ToString().ToLowerInvariant())
 foreach($v in $ChangedFiles){$arguments+=@('--changed-file',$v)}
 foreach($v in $ChangedModules){$arguments+=@('--changed-module',$v)}
 foreach($v in $FeatureTags){$arguments+=@('--tag',$v)}
 foreach($v in $RiskOverride){$arguments+=@('--risk',$v)}
 if($BudgetOverrideReason){$arguments+=@('--budget-override-reason',$BudgetOverrideReason)}
}
if($OutputFile){$arguments+=@('--output',[IO.Path]::GetFullPath($OutputFile))}
& node @arguments
exit $LASTEXITCODE
