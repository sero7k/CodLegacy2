# Claude's BO3 tour: each line of bo3_maps.txt is `tag MAP=t7:mp_x`. Three shots per map from the
# spawn: facing as spawned, then turned 120 and 240 degrees (cheats on for `look`).
# Shots go to out_bo3\shots\<tag>_1..3.png, logs to out_bo3\game-<tag>.log.
$ErrorActionPreference = 'Continue'
$Out  = Join-Path $PSScriptRoot 'out_bo3'
$Play = 'F:\Call of Duty Claude\play'
New-Item -ItemType Directory -Force "$Out\shots" | Out-Null
$lines = Get-Content (Join-Path $PSScriptRoot 'bo3_maps.txt') | Where-Object { $_.Trim() -and -not $_.StartsWith('#') }
foreach ($line in $lines) {
  $parts = $line.Trim() -split '\s+'
  $tag = $parts[0]
  $map = ($parts[1] -replace '^MAP=','')
  "== $tag ($map)"
  Get-Process -Name 'iw4l-claude' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  $since = Get-Date
  $cmds = "wait world; wait 25s; screenshot ${tag}_1; wait 2s; look 120 0; wait 2s; screenshot ${tag}_2; wait 2s; look 240 0; wait 2s; screenshot ${tag}_3; wait 2s; finish_run"
  $p = Start-Process -FilePath "$Play\iw4l-claude.exe" -ArgumentList @('map', $map, '--cmds', "`"$cmds`"") -WorkingDirectory $Play -PassThru
  if (-not $p.WaitForExit(300000)) { "  still running after 5 min: closed"; Get-Process -Name 'iw4l-claude' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue }
  "  exit $($p.ExitCode)"
  Get-ChildItem "$Play\codlegacy-artifacts" -Recurse -Filter "${tag}_*.png" -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $since } | ForEach-Object { Copy-Item $_.FullName "$Out\shots\$($_.Name)" -Force; "  shot $($_.Name)" }
  Copy-Item "$Play\codlegacy-artifacts\logs\latest.log" "$Out\game-$tag.log" -Force -ErrorAction SilentlyContinue
}
