param(
  [string]$ForkRemote = "origin",
  [string]$ForkBranch = "backup/all-fork-changes",
  [string]$UpstreamRemote = "upstream",
  [string]$UpstreamBranch = "beta",
  [switch]$NoFetch,
  [switch]$Json
)

$ErrorActionPreference = "Stop"

function Invoke-Git {
  param([Parameter(Mandatory = $true)][string[]]$Args)
  $output = & git @Args 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "git $($Args -join ' ') failed:`n$output"
  }
  return $output
}

function Get-GitText {
  param([Parameter(Mandatory = $true)][string[]]$Args)
  $output = Invoke-Git -Args $Args
  return ($output -join "`n").Trim()
}

function Test-GitRef {
  param([Parameter(Mandatory = $true)][string]$Ref)
  & git rev-parse --verify --quiet $Ref *> $null
  return $LASTEXITCODE -eq 0
}

function Test-GitRemote {
  param([Parameter(Mandatory = $true)][string]$Remote)
  & git remote get-url $Remote *> $null
  return $LASTEXITCODE -eq 0
}

function Get-GitDirPath {
  param([Parameter(Mandatory = $true)][string]$RepoRoot)
  $gitDir = Get-GitText -Args @("rev-parse", "--git-dir")
  if ([System.IO.Path]::IsPathRooted($gitDir)) {
    return $gitDir
  }
  return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $gitDir))
}

function Test-IntegrationInProgress {
  param([Parameter(Mandatory = $true)][string]$GitDir)
  $markers = @(
    "rebase-merge",
    "rebase-apply",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD"
  )
  foreach ($marker in $markers) {
    if (Test-Path -LiteralPath (Join-Path $GitDir $marker)) {
      return $marker
    }
  }
  return $null
}

$repoRoot = Get-GitText -Args @("rev-parse", "--show-toplevel")
Set-Location -LiteralPath $repoRoot
$gitDir = Get-GitDirPath -RepoRoot $repoRoot

$inProgress = Test-IntegrationInProgress -GitDir $gitDir
if ($inProgress) {
  throw "Git integration is already in progress ($inProgress). Finish or abort it before checking upstream."
}

foreach ($remote in @($ForkRemote, $UpstreamRemote)) {
  if (-not (Test-GitRemote -Remote $remote)) {
    throw "Git remote '$remote' is not configured. Add it first, then rerun this check."
  }
}

if (-not $NoFetch) {
  Invoke-Git -Args @("fetch", $ForkRemote, $ForkBranch) | Out-Null
  Invoke-Git -Args @("fetch", $UpstreamRemote, $UpstreamBranch, "--tags") | Out-Null
}

$forkRef = "$ForkRemote/$ForkBranch"
$upstreamRef = "$UpstreamRemote/$UpstreamBranch"

if (-not (Test-GitRef -Ref $upstreamRef)) {
  throw "Upstream ref was not found: $upstreamRef. If the upstream default branch changed, rerun with -UpstreamBranch <branch>."
}

$hasForkRef = Test-GitRef -Ref $forkRef
$branch = Get-GitText -Args @("branch", "--show-current")
$head = Get-GitText -Args @("log", "-1", "--format=%h %cI %s", "HEAD")
$upstreamHead = Get-GitText -Args @("log", "-1", "--format=%h %cI %s", $upstreamRef)
$aheadOfUpstream = [int](Get-GitText -Args @("rev-list", "--count", "$upstreamRef..HEAD"))
$behindUpstream = [int](Get-GitText -Args @("rev-list", "--count", "HEAD..$upstreamRef"))
$trackedStatus = @(Invoke-Git -Args @("status", "--porcelain", "--untracked-files=no"))
$untrackedStatus = @(Invoke-Git -Args @("status", "--porcelain", "--untracked-files=normal") | Where-Object { $_ -like "??*" })
$tags = @(Invoke-Git -Args @("tag", "--sort=-v:refname") | Select-Object -First 5)

$forkBehind = $null
$forkAhead = $null
if ($hasForkRef) {
  $forkBehind = [int](Get-GitText -Args @("rev-list", "--count", "HEAD..$forkRef"))
  $forkAhead = [int](Get-GitText -Args @("rev-list", "--count", "$forkRef..HEAD"))
}

$result = [ordered]@{
  repoRoot = $repoRoot
  currentBranch = $branch
  expectedForkBranch = $ForkBranch
  currentHead = $head
  upstreamRef = $upstreamRef
  upstreamHead = $upstreamHead
  aheadOfUpstream = $aheadOfUpstream
  behindUpstream = $behindUpstream
  forkRef = if ($hasForkRef) { $forkRef } else { $null }
  aheadOfFork = $forkAhead
  behindFork = $forkBehind
  trackedChanges = $trackedStatus.Count
  untrackedChanges = $untrackedStatus.Count
  latestTagsByVersion = $tags
  fetched = -not $NoFetch
  mutatesWorkingTree = $false
  mutatesOpenLessSettings = $false
  mergesOrPushes = $false
}

if ($Json) {
  $result | ConvertTo-Json -Depth 4
  exit 0
}

Write-Host "OpenLess fork upstream check"
Write-Host "This script fetches Git remote refs only. It does not merge, push, edit files, or change OpenLess settings."
Write-Host "Repo: $repoRoot"
Write-Host "Branch: $branch"
Write-Host "HEAD: $head"
Write-Host "Compare target: $upstreamRef"
Write-Host "Upstream HEAD: $upstreamHead"
Write-Host "Compared with upstream: behind $behindUpstream / ahead $aheadOfUpstream"
if ($hasForkRef) {
  Write-Host "Compared with fork: behind $forkBehind / ahead $forkAhead"
} else {
  Write-Host "Compared with fork: $forkRef was not found"
}

if ($tags.Count -gt 0) {
  Write-Host "Latest tags by version-like sort:"
  foreach ($tag in $tags) {
    Write-Host "  - $tag"
  }
}

if ($branch -ne $ForkBranch) {
  Write-Host "[warn] Current branch is not $ForkBranch."
}
if ($trackedStatus.Count -gt 0) {
  Write-Host "[warn] Tracked working-tree changes exist: $($trackedStatus.Count)"
}
if ($untrackedStatus.Count -gt 0) {
  Write-Host "[info] Untracked files exist: $($untrackedStatus.Count)"
}

if ($behindUpstream -eq 0) {
  Write-Host "[ok] No new commits from $upstreamRef are pending."
} else {
  Write-Host "[review] $behindUpstream upstream commit(s) are not integrated yet."
  Write-Host "Run: git log --oneline --decorate HEAD..$upstreamRef"
}
