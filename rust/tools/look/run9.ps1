# Claude's all-maps look runner: each line of maps9.txt is `tag KEY=VALUE ...` (MAP=h1:mp_x).
# No class is picked, so the view stays on the map's intermission camera (where HorizonMW's
# reference shots were taken). One screenshot per map into out9\shots, logs into out9.
$ErrorActionPreference = 'Continue'
$Out  = Join-Path $PSScriptRoot 'out9'
$Play = 'F:\Call of Duty Claude\play'
New-Item -ItemType Directory -Force "$Out\shots" | Out-Null
$lines = Get-Content (Join-Path $PSScriptRoot 'maps9.txt') | Where-Object { $_.Trim() -and -not $_.StartsWith('#') }
foreach ($line in $lines) {
  $parts = $line.Trim() -split '\s+'
  $tag = $parts[0]
  $set = @()
  foreach ($kv in $parts[1..($parts.Length)]) { if ($kv -match '^([A-Z0-9_]+)=(.*)$') { $set += ,@($Matches[1], $Matches[2]) } }
  foreach ($s in $set) { Set-Item "env:$($s[0])" $s[1] }
  Remove-Item env:BO2MP_AUTOCLASS -ErrorAction SilentlyContinue
  "== $tag : $($set | ForEach-Object { $_ -join '=' })"
  Get-Process -Name 'iw4l-claude' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  $since = Get-Date
  $cmds = "wait world; wait 30s; screenshot iv_$tag; wait 2s; finish_run"
  $p = Start-Process -FilePath "$Play\iw4l-claude.exe" -ArgumentList @('map', $env:MAP, '--cmds', "`"$cmds`"") -WorkingDirectory $Play -PassThru
  if (-not $p.WaitForExit(300000)) {
    "  still running after 5 min: closed"
    Get-Process -Name 'iw4l-claude' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  }
  "  exit $($p.ExitCode)"
  Get-ChildItem "$Play\codlegacy-artifacts" -Recurse -Filter "iv_$tag.png" -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $since } | ForEach-Object { Copy-Item $_.FullName "$Out\shots\$($_.Name)" -Force; "  shot $($_.Name)" }
  Copy-Item "$Play\codlegacy-artifacts\logs\latest.log" "$Out\game-$tag.log" -Force -ErrorAction SilentlyContinue
  foreach ($s in $set) { Remove-Item "env:$($s[0])" -ErrorAction SilentlyContinue }
}
