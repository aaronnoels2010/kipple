# SPDX-License-Identifier: GPL-3.0-or-later
#
# Spike S8 on Windows: idle memory of the spike app in each mode (open / hide / destroy).
# Sums the app and its WebView2 processes (msedgewebview2.exe children): working set and
# private bytes (commit).
#
#   pwsh spikes/s8-idle-memory/measure.ps1        # $env:RUNS=3 $env:IDLE=30 by default
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$bin = "src-tauri\target\release\kipple-spike-s8.exe"
$runs = if ($env:RUNS) { [int]$env:RUNS } else { 3 }
$idle = if ($env:IDLE) { [int]$env:IDLE } else { 30 }
New-Item -ItemType Directory -Force out | Out-Null

function Get-Tree([int]$root) {
  $all = Get-CimInstance Win32_Process
  $ids = @($root)
  $frontier = @($root)
  while ($frontier.Count -gt 0) {
    $children = @($all | Where-Object { $frontier -contains $_.ParentProcessId } | ForEach-Object { [int]$_.ProcessId })
    $ids += $children
    $frontier = $children
  }
  $all | Where-Object { $ids -contains $_.ProcessId }
}

$rows = @()
foreach ($mode in "open", "hide", "destroy") {
  foreach ($run in 1..$runs) {
    $env:S8_MODE = $mode
    $log = "out\$mode-$run.log"
    $p = Start-Process -FilePath $bin -PassThru -RedirectStandardError $log
    Start-Sleep -Seconds (8 + $idle)
    $tree = @(Get-Tree $p.Id)
    $ws = ($tree | Measure-Object -Property WorkingSetSize -Sum).Sum
    $priv = ($tree | Measure-Object -Property PrivatePageCount -Sum).Sum
    # Only count runs where the hide/destroy really happened (the app logs it).
    $valid = ($mode -eq "open") -or [bool](Select-String -Path $log -Pattern "^S8: $mode -> Ok" -Quiet)
    $rows += [pscustomobject]@{ Mode = $mode; Run = $run; Processes = $tree.Count; WorkingSet = $ws; Private = $priv; Valid = $valid }
    "$mode`t$run`t$($tree.Count)`t$ws`t$priv`t$valid"
    $tree | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 2
  }
}

$md = @("### Windows ($runs runs, ${idle}s idle, median)", "", "| Mode | Processes | Working set (MiB) | Private bytes (MiB) | Valid runs |", "|---|---|---|---|---|")
foreach ($mode in "open", "hide", "destroy") {
  $sorted = @($rows | Where-Object { $_.Mode -eq $mode -and $_.Valid } | Sort-Object WorkingSet)
  if ($sorted.Count -eq 0) { $md += "| $mode | – | – | – | 0/$runs |"; continue }
  $m = $sorted[[int][math]::Floor(($sorted.Count - 1) / 2)]
  $md += "| $mode | $($m.Processes) | $([math]::Round($m.WorkingSet / 1MB)) | $([math]::Round($m.Private / 1MB)) | $($sorted.Count)/$runs |"
}
$md | Tee-Object -FilePath out\results-Windows.md
