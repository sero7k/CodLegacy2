# Portable export of the BO2 rebuild: one folder holding CodLegacy.exe, its settings
# and trimmed copies of every game's files the game reads, so it runs with
# none of the original game folders present. See docs/EXPORT.md.
#
#   export-portable.ps1 -DryRun              the plan and sizes only, nothing written
#   export-portable.ps1                      export to the default folder, then verify
#   export-portable.ps1 -Destination E:\BO2_Rebuild -NoBuild -SkipVerify
#
# It never deletes or changes anything outside the destination, and never
# deletes anything inside it either: a file left from an earlier export that
# the rules no longer keep is listed in the report, not removed.
param(
    [string]$Destination = '',
    [switch]$DryRun,
    [switch]$NoBuild,
    [string]$BuildProfile = 'play',
    [string]$PlayDir = '',
    [string]$RepoDir = '',
    [string]$GamePaths = '',
    [string[]]$Only = @(),
    [string[]]$Skip = @(),
    [switch]$IncludeCache,
    [switch]$RedoJup,
    [switch]$SkipVerify,
    [switch]$VerifyOnly,
    [switch]$VerifyMaps,
    [int]$VerifyMinutes = 30
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

function Say($t) { Write-Host "`n== $t" -ForegroundColor Cyan }
function Warn($t) { Write-Host "WARNING: $t" -ForegroundColor Yellow; $script:Warnings.Add($t) | Out-Null }
function Gb([double]$b) { '{0,8:N2} GB' -f ($b / 1GB) }
$script:Warnings = New-Object System.Collections.ArrayList
$script:Report = New-Object System.Collections.ArrayList
function Note($t) { Write-Host $t; $script:Report.Add($t) | Out-Null }

# --- where things are -------------------------------------------------------
if (-not $PlayDir) { $PlayDir = Join-Path $env:USERPROFILE 'bo2\play' }
if (-not $GamePaths) { $GamePaths = Join-Path $PlayDir 'game_paths.cfg' }
if (-not $RepoDir) {
    $here = Resolve-Path (Join-Path $PSScriptRoot '..\..') -ErrorAction SilentlyContinue
    if ($here -and (Test-Path (Join-Path $here 'Cargo.toml'))) { $RepoDir = $here.Path }
    else { $RepoDir = Join-Path $env:USERPROFILE 'bo2\bo2-multiplayer-rebuild' }
}
$RulesFile = Join-Path $PSScriptRoot 'rules.json'
if (-not (Test-Path $RulesFile)) { throw "rules.json not found beside the script ($RulesFile)" }
$Rules = Get-Content -Raw -LiteralPath $RulesFile | ConvertFrom-Json

# The order of game_paths.cfg's lines (asset_transport::game_paths::GAMES).
$Keys = @('bo2','bo1','mw2','mw3','bo3','aw','jup','waw','cod4','ghosts','iw','mwr','bf1','cw','vg','bo2mod','bo2extra')

# Settings that name a folder or file outside the export: never carried into
# the export's .env (the export sets its own where it needs one).
$PathKeys = @('CODLEGACY_GAMES','CODLEGACY_GAME_PATHS','CODLEGACY_T6_MOD','CODLEGACY_T6_EXTRA','CODLEGACY_AW_DIR','CODLEGACY_GHOSTS_DIR',
    'CODLEGACY_HMW_ZONE','CODLEGACY_MUSIC_DIR','CODLEGACY_MUSIC_CFG','CODLEGACY_VOICE_GAMES','CODLEGACY_SETTINGS_PATH','CODLEGACY_TRACES_DIR',
    'CODLEGACY_BO3_WORKSHOP','CODLEGACY_NO_DETECT','BO2_PROFILE_DIR','BO2_JUP_INSTALL','BO2_JUP_EXPORT','BF1_FILES',
    'BF1_GAME_DIR','BO2MP_REPO_DIR','BO2MP_TABLES_DIR','T7_ZONE_DIR','S1_ZONE_DIR','H1_ZONE_DIR','JUP_ZONE_DIR',
    'IW6_ZONE_DIR','JUP_PACK_DIR')
# Environment variables the game reads that could point a verify run at the
# original folders: cleared for every run the script starts.
$ScrubPattern = '^(CODLEGACY_|BO2_|BO2MP_|BF1_|T6_|T7_|H1_|S1_|IW6_|JUP_)'

# --- glob patterns (crates/call_of_claude/src/glob.rs semantics) ------------
$RegexCache = @{}
function GlobRegex([string]$pattern) {
    if ($RegexCache.ContainsKey($pattern)) { return $RegexCache[$pattern] }
    $segs = $pattern.ToLowerInvariant().Split('/')
    $r = '^'
    for ($i = 0; $i -lt $segs.Count; $i++) {
        $s = $segs[$i]; $last = ($i -eq $segs.Count - 1)
        if ($s -eq '**') { if ($last) { $r += '.*' } else { $r += '(?:[^/]+/)*' }; continue }
        $r += ([regex]::Escape($s) -replace '\\\*', '[^/]*')
        if (-not $last) { $r += '/' }
    }
    $rx = New-Object System.Text.RegularExpressions.Regex ($r + '$'), 'IgnoreCase, CultureInvariant'
    $RegexCache[$pattern] = $rx
    return $rx
}
function AnyMatch($patterns, [string]$rel) {
    foreach ($p in $patterns) { if ((GlobRegex $p).IsMatch($rel)) { return $true } }
    return $false
}
function Prop($obj, [string]$name) {
    if ($null -eq $obj) { return $null }
    $p = $obj.PSObject.Properties[$name]
    if ($p) { return $p.Value } else { return $null }
}

# --- the source folders ------------------------------------------------------
function Read-KeyFile([string]$path) {
    $map = [ordered]@{}
    if (-not (Test-Path -LiteralPath $path)) { return $map }
    foreach ($line in Get-Content -LiteralPath $path) {
        $t = $line.Trim()
        if (-not $t -or $t.StartsWith('#')) { continue }
        $i = $t.IndexOf('=')
        if ($i -lt 1) { continue }
        $k = $t.Substring(0, $i).Trim()
        $v = $t.Substring($i + 1).Trim().Trim('"')
        $map[$k] = $v
    }
    return $map
}
$SourceCfg = Read-KeyFile $GamePaths
$SourceEnv = Read-KeyFile (Join-Path $PlayDir '.env')
if (-not $SourceCfg.Contains('bo2') -or -not $SourceCfg['bo2']) { throw "No bo2 line in ${GamePaths}: point -GamePaths at the game_paths.cfg the game uses." }

# The mods: as the game resolves them today (game_paths, then .env, then the
# mod's default folder), so the export holds what is in use now.
$Sources = [ordered]@{}
foreach ($k in $Keys) {
    $v = if ($SourceCfg.Contains($k)) { $SourceCfg[$k] } else { '' }
    if ($v) { $Sources[$k] = $v }
}
if (-not $Sources.Contains('bo2mod')) {
    $m = $SourceEnv['CODLEGACY_T6_MOD']
    if ($m -and $m -notin @('0','off')) { $Sources['bo2mod'] = $m }
    elseif (-not $m -and (Test-Path 'E:\mp_leetin_pub\mod.ff')) { $Sources['bo2mod'] = 'E:\mp_leetin_pub' }
}
if (-not $Sources.Contains('bo2extra') -and $SourceEnv['CODLEGACY_T6_EXTRA']) {
    $first = ($SourceEnv['CODLEGACY_T6_EXTRA'] -split ';')[0]
    if ($first) { $Sources['bo2extra'] = $first }
}

$Games = @()
foreach ($k in $Keys) {
    if (-not $Sources.Contains($k)) { continue }
    if ($Only.Count -and $k -notin $Only) { continue }
    if ($k -in $Skip) { continue }
    $rule = Prop $Rules.games $k
    if (-not $rule) { Note ("{0,-8} not exported: no rules for it (bf1 and removed games are left out)" -f $k); continue }
    $src = $Sources[$k] -replace '/', '\'
    if (-not (Test-Path -LiteralPath $src -PathType Container)) { Warn "$k folder $src is not there: not exported"; continue }
    $Games += [pscustomobject]@{ Key = $k; Source = (Resolve-Path -LiteralPath $src).Path.TrimEnd('\'); Rule = $rule }
}

# --- destination ----------------------------------------------------------
function Free-Bytes([string]$path) {
    $root = [System.IO.Path]::GetPathRoot([System.IO.Path]::GetFullPath($path))
    try { return (New-Object System.IO.DriveInfo $root).AvailableFreeSpace } catch { return 0 }
}
if (-not $Destination) {
    $disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | Sort-Object FreeSpace -Descending)
    Say 'Drives'
    foreach ($d in $disks) { Write-Host ('{0}  {1} free of {2}  {3}' -f $d.DeviceID, (Gb $d.FreeSpace), (Gb $d.Size), $d.VolumeName) }
    # An export made before is updated in place (only what changed is copied).
    $docs = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'BO2_Rebuild'
    $earlier = @(@($disks | ForEach-Object { Join-Path ($_.DeviceID + '\') 'BO2_Rebuild' }) + $docs | Where-Object { (Test-Path -LiteralPath (Join-Path $_ 'CodLegacy.exe')) -or (Test-Path -LiteralPath (Join-Path $_ 'iw4l.exe')) })
    if ($earlier.Count) { $Destination = $earlier[0]; Write-Host "An earlier export is at ${Destination}: updating it (-Destination picks another folder)." }
    else {
        $best = $disks | Select-Object -First 1
        if (-not $best) { throw 'No local drive found.' }
        if ($best.DeviceID -ieq $env:SystemDrive) { $Destination = $docs }
        else { $Destination = Join-Path ($best.DeviceID + '\') 'BO2_Rebuild' }
    }
}
$Destination = [System.IO.Path]::GetFullPath($Destination).TrimEnd('\')
foreach ($g in $Games) {
    if ($Destination.StartsWith($g.Source + '\', 'OrdinalIgnoreCase') -or $Destination -ieq $g.Source) { throw "The destination $Destination is inside $($g.Key)'s folder $($g.Source): pick another." }
    if ($g.Source.StartsWith($Destination + '\', 'OrdinalIgnoreCase')) { throw "$($g.Key)'s folder $($g.Source) is inside the destination: pick another." }
}
$GamesDir = Join-Path $Destination 'games'
Say "Export folder: $Destination"

# --- the plan ----------------------------------------------------------------
function Plan-Game($g) {
    $rule = $g.Rule
    $method = Prop $rule 'method'
    $keep = @(Prop $rule 'keep'); $drop = @($Rules.common_drop) + @(Prop $rule 'drop'); $force = @(Prop $rule 'force')
    $files = if ($method -eq 'export-mw3') { @() }
        elseif ($method -eq 'banner') { @(Get-ChildItem -LiteralPath $g.Source -File -Force -ErrorAction SilentlyContinue) }
        else { @(Get-ChildItem -LiteralPath $g.Source -Recurse -File -Force -ErrorAction SilentlyContinue) }
    $base = $g.Source.Length + 1
    $rels = foreach ($f in $files) { [pscustomobject]@{ File = $f; Rel = $f.FullName.Substring($base).Replace('\', '/') } }
    $maps = @()
    $marker = Prop $rule 'map_marker'
    if ($marker) {
        $maps = @($rels | Where-Object { (GlobRegex $marker).IsMatch($_.Rel) } | ForEach-Object { $_.File.Name -replace '(?i)_load\.ff$', '' } | Sort-Object -Unique)
    }
    $mapZones = @(foreach ($m in $maps) { foreach ($z in @(Prop $rule 'map_zones')) { if ($z) { $z.Replace('{map}', $m) } } })
    $kept = New-Object System.Collections.ArrayList
    [long]$keptBytes = 0; [long]$skipBytes = 0; [long]$mapBytes = 0; [long]$todoBytes = 0
    foreach ($r in $rels) {
        $len = $r.File.Length
        $ok = $false
        if ($method -eq 'banner') { $ok = $r.Rel -match '^(?i)banner\.(png|jpe?g)$' }
        elseif ($method -eq 'export-mw3') { $ok = $false }
        elseif (AnyMatch $keep $r.Rel) {
            $isMap = $mapZones.Count -and (AnyMatch $mapZones $r.Rel)
            $ok = (AnyMatch $force $r.Rel) -or -not ((AnyMatch $drop $r.Rel) -or $isMap)
            if ($isMap -and -not $ok) { $mapBytes += $len }
        }
        if ($ok) {
            $kept.Add($r) | Out-Null; $keptBytes += $len
            $to = Join-Path (Join-Path $GamesDir $g.Key) $r.Rel.Replace('/', '\')
            $have = Get-Item -LiteralPath $to -ErrorAction SilentlyContinue
            if (-not ($have -and $have.Length -eq $len -and $have.LastWriteTimeUtc -eq $r.File.LastWriteTimeUtc)) { $todoBytes += $len }
        } else { $skipBytes += $len }
    }
    [pscustomobject]@{ Game = $g; Kept = $kept; KeptBytes = $keptBytes; SkipBytes = $skipBytes; MapBytes = $mapBytes; Maps = $maps; TodoBytes = $todoBytes; Method = $method }
}

$Plans = @()
if (-not $VerifyOnly) {
    Say 'Plan (kept = copied into the export, skipped = left out)'
    foreach ($g in $Games) {
        Write-Host "  reading $($g.Key): $($g.Source) ..."
        $p = Plan-Game $g
        $Plans += $p
    }
    [long]$totKeep = 0; [long]$totSkip = 0; [long]$totTodo = 0
    Note ''
    Note ('{0,-8} {1,11} {2,11} {3,11}  {4}' -f 'game', 'kept', 'skipped', 'to copy', 'notes')
    foreach ($p in $Plans) {
        $notes = $p.Game.Rule.title
        if ($p.Maps.Count) { $notes += "; $($p.Maps.Count) maps left out ($(Gb $p.MapBytes) of map zones)" }
        if ($p.Method -eq 'export-mw3') { $notes += '; made by CodLegacy.exe export-mw3 (guns only, a few GB)' }
        Note ('{0,-8} {1} {2} {3}  {4}' -f $p.Game.Key, (Gb $p.KeptBytes), (Gb $p.SkipBytes), (Gb $p.TodoBytes), $notes)
        $totKeep += $p.KeptBytes; $totSkip += $p.SkipBytes; $totTodo += $p.TodoBytes
    }
    Note ('{0,-8} {1} {2} {3}' -f 'total', (Gb $totKeep), (Gb $totSkip), (Gb $totTodo))
    $mw3 = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'MW3_export'
    if (Test-Path -LiteralPath (Join-Path $mw3 'manifest.json')) {
        $mb = (Get-ChildItem -LiteralPath $mw3 -Recurse -File | Measure-Object Length -Sum).Sum
        Note ("MW3_export (Modern Warfare III's converted guns, $mw3): $(Gb $mb) copied too")
        $totTodo += $mb
    }
    $free = Free-Bytes $Destination
    Note ("Free on the destination drive: $(Gb $free); needed now: $(Gb $totTodo) plus the exe and the MW3 copy.")
    if ($free -lt ($totTodo + 10GB)) { Warn "Not enough room on $([System.IO.Path]::GetPathRoot($Destination)) for the export (need about $(Gb ($totTodo + 10GB)) with 10 GB to spare)." }
    $leftOut = @($Keys | Where-Object { $Sources.Contains($_) -and -not ($Games | Where-Object Key -eq $_) })
    if ($leftOut.Count) { Note ("Set in game_paths.cfg but not exported: " + ($leftOut -join ', ')) }
    if ($DryRun) {
        if ($script:Warnings.Count) { Say 'Warnings'; $script:Warnings | ForEach-Object { Write-Host "  $_" } }
        Say 'Dry run: nothing was written.'
        return
    }
    if ($free -lt $totTodo) { throw 'Not enough free space: free some room or pass -Destination on another drive.' }
}

New-Item -ItemType Directory -Force -Path $Destination, $GamesDir | Out-Null

# --- the game ----------------------------------------------------------------
$ExeDest = Join-Path $Destination 'CodLegacy.exe'
if (-not $VerifyOnly) {
    Say 'The game (CodLegacy.exe)'
    $built = $false
    if (-not $NoBuild -and (Test-Path (Join-Path $RepoDir 'Cargo.toml')) -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "Building from $RepoDir (cargo build -p launcher --profile $BuildProfile) ..."
        Push-Location $RepoDir
        try { & cargo build -p launcher --profile $BuildProfile; $built = ($LASTEXITCODE -eq 0) } finally { Pop-Location }
        if (-not $built) { Warn 'The build failed: the newest CodLegacy.exe already built is used instead.' }
    }
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoDir 'target' }
    $cands = @((Join-Path $target "$BuildProfile\CodLegacy.exe"), (Join-Path $target 'play\CodLegacy.exe'), (Join-Path $target 'release\CodLegacy.exe'), (Join-Path $PlayDir 'CodLegacy.exe')) |
        Where-Object { Test-Path -LiteralPath $_ } | ForEach-Object { Get-Item -LiteralPath $_ } | Sort-Object LastWriteTime -Descending
    $exe = $cands | Select-Object -First 1
    if (-not $exe) { throw "No CodLegacy.exe found (built in $target or in $PlayDir)." }
    Note "CodLegacy.exe: $($exe.FullName) ($($exe.LastWriteTime))"
    Copy-Item -LiteralPath $exe.FullName -Destination $ExeDest -Force
    # the game used to be iw4l.exe: an export updated in place ends with only CodLegacy.exe
    $oldExe = Join-Path $Destination 'iw4l.exe'
    if (Test-Path -LiteralPath $oldExe) { Remove-Item -LiteralPath $oldExe -Force }
    foreach ($legal in @('LICENSE', 'NOTICE', 'crates\ui\assets\OFL-BarlowCondensed.txt', 'crates\console\assets\COPYING-FreeFont.txt')) {
        $p = Join-Path $RepoDir $legal
        if (Test-Path -LiteralPath $p) { Copy-Item -LiteralPath $p -Destination $Destination -Force }
    }
}

# --- the games' files --------------------------------------------------------
function Invoke-CodLegacy([string]$exe, [string[]]$arguments, [hashtable]$vars, [string]$out, [int]$minutes, [string]$fakeHome) {
    $saved = @{}
    foreach ($name in @([Environment]::GetEnvironmentVariables().Keys)) {
        if ($name -match $ScrubPattern) { $saved[$name] = [Environment]::GetEnvironmentVariable($name); [Environment]::SetEnvironmentVariable($name, $null) }
    }
    $homeVars = @('USERPROFILE', 'HOME')
    if ($fakeHome) {
        foreach ($h in $homeVars) { $saved[$h] = [Environment]::GetEnvironmentVariable($h); [Environment]::SetEnvironmentVariable($h, $fakeHome) }
        New-Item -ItemType Directory -Force -Path $fakeHome | Out-Null
    }
    foreach ($kv in $vars.GetEnumerator()) { if (-not $saved.ContainsKey($kv.Key)) { $saved[$kv.Key] = [Environment]::GetEnvironmentVariable($kv.Key) }; [Environment]::SetEnvironmentVariable($kv.Key, $kv.Value) }
    try {
        $err = "$out.err"
        $proc = Start-Process -FilePath $exe -ArgumentList ($arguments | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }) `
            -WorkingDirectory (Split-Path $exe) -RedirectStandardOutput $out -RedirectStandardError $err -NoNewWindow -PassThru
        $null = $proc.Handle
        if (-not $proc.WaitForExit($minutes * 60 * 1000)) { $proc.Kill(); Warn "$exe $($arguments -join ' ') ran past $minutes minutes and was stopped"; return 124 }
        return $proc.ExitCode
    } finally {
        foreach ($kv in $saved.GetEnumerator()) { [Environment]::SetEnvironmentVariable($kv.Key, $kv.Value) }
    }
}

function Robo([string]$from, [string]$to, [string[]]$names, [string[]]$extra) {
    $rargs = @($from, $to) + $names + @('/COPY:DAT', '/DCOPY:T', '/R:2', '/W:3', '/J', '/NP', '/NJH', '/NJS', '/NDL', '/NFL') + $extra
    & robocopy @rargs | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE): $from -> $to" }
}
if (-not $VerifyOnly) {
    foreach ($p in $Plans) {
        $k = $p.Game.Key
        if ($p.Method -eq 'export-mw3') { continue }
        if (-not $p.Kept.Count) { continue }
        Say "Copying $k ($(Gb $p.TodoBytes) to copy of $(Gb $p.KeptBytes))"
        $byDir = $p.Kept | Group-Object { [System.IO.Path]::GetDirectoryName($_.Rel.Replace('/', '\')) }
        foreach ($grp in $byDir) {
            $from = if ($grp.Name) { Join-Path $p.Game.Source $grp.Name } else { $p.Game.Source }
            $to = if ($grp.Name) { Join-Path (Join-Path $GamesDir $k) $grp.Name } else { Join-Path $GamesDir $k }
            $batch = New-Object System.Collections.ArrayList; $len = 0
            foreach ($r in $grp.Group) {
                $n = $r.File.Name
                if ($batch.Count -and ($len + $n.Length + 3) -gt 6000) { Robo $from $to $batch.ToArray() @(); $batch.Clear(); $len = 0 }
                $batch.Add($n) | Out-Null; $len += $n.Length + 3
            }
            if ($batch.Count) { Robo $from $to $batch.ToArray() @() }
        }
        if ($k -in @('cw', 'vg')) {
            $b = Get-ChildItem -LiteralPath (Join-Path $GamesDir $k) -File -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($b) {
                New-Item -ItemType Directory -Force -Path (Join-Path $Destination 'banners') | Out-Null
                Copy-Item -LiteralPath $b.FullName -Destination (Join-Path $Destination "banners\$k$($b.Extension)") -Force
            }
        }
    }
    # Files from an earlier export that the rules no longer keep: listed only.
    foreach ($p in $Plans) {
        $dir = Join-Path $GamesDir $p.Game.Key
        if ($p.Method -eq 'export-mw3' -or -not (Test-Path $dir)) { continue }
        $want = @{}; foreach ($r in $p.Kept) { $want[$r.Rel.ToLowerInvariant()] = $true }
        $stale = @(Get-ChildItem -LiteralPath $dir -Recurse -File | Where-Object { -not $want.ContainsKey($_.FullName.Substring($dir.Length + 1).Replace('\', '/').ToLowerInvariant()) })
        if ($stale.Count) { Warn "$($p.Game.Key): $($stale.Count) files ($(Gb ($stale | Measure-Object Length -Sum).Sum)) in the export are no longer kept by the rules; delete them by hand if you want the room (not deleted)." }
    }
}

# --- configuration -----------------------------------------------------------
$Exported = @{}
function Update-Exported {
    $script:Exported = @{}
    foreach ($k in $Keys) {
        $d = Join-Path $GamesDir $k
        if ((Test-Path -LiteralPath $d) -and (Get-ChildItem -LiteralPath $d -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 1)) { $script:Exported[$k] = $true }
    }
}
function Write-Config([string]$dir, [bool]$absolute) {
    Update-Exported
    $prefix = if ($absolute) { $GamesDir } else { 'games' }
    $cfg = "# Game folders (key=folder). Set in the game: Game Folders.`r`n"
    foreach ($k in $Keys) {
        $v = if ($Exported.Contains($k) -and $k -notin @('cw', 'vg')) { "$prefix\$k" } else { '' }
        $cfg += "$k=$v`r`n"
    }
    [System.IO.File]::WriteAllText((Join-Path $dir 'game_paths.cfg'), $cfg, (New-Object System.Text.UTF8Encoding $false))
    $fwd = $prefix.Replace('\', '/')
    $lines = New-Object System.Collections.ArrayList
    $lines.Add('# Written by tools/export/export-portable.ps1: every folder here is inside this export.') | Out-Null
    foreach ($kv in $SourceEnv.GetEnumerator()) {
        if ($kv.Key -in $PathKeys -or $kv.Value -match '^[A-Za-z]:[\\/]' -or $kv.Value -match '^\\\\') {
            $lines.Add("# left out (a folder outside the export): $($kv.Key)") | Out-Null
            continue
        }
        $lines.Add("$($kv.Key)=$($kv.Value)") | Out-Null
    }
    $lines.Add("CODLEGACY_GAMES=$fwd/bo2") | Out-Null
    $lines.Add('CODLEGACY_NO_DETECT=1') | Out-Null
    $lines.Add('CODLEGACY_BO3_WORKSHOP=0') | Out-Null
    if ($Exported.Contains('bo2mod')) { $lines.Add("CODLEGACY_T6_MOD=$fwd/bo2mod") | Out-Null } else { $lines.Add('CODLEGACY_T6_MOD=off') | Out-Null }
    if ($Exported.Contains('bo2extra')) { $lines.Add("CODLEGACY_T6_EXTRA=$fwd/bo2extra") | Out-Null }
    $mw3 = Join-Path $Destination 'MW3_export'
    if (Test-Path -LiteralPath $mw3) {
        $v = if ($absolute) { $mw3.Replace('\', '/') } else { 'MW3_export' }
        $lines.Add("BO2_JUP_EXPORT=$v") | Out-Null
    }
    [System.IO.File]::WriteAllLines((Join-Path $dir '.env'), [string[]]$lines.ToArray(), (New-Object System.Text.UTF8Encoding $false))
}

if (-not $VerifyOnly) {
    # Modern Warfare III (2023) and II (2022): the game's own small copy.
    $jup = $Plans | Where-Object { $_.Method -eq 'export-mw3' } | Select-Object -First 1
    $jupDir = Join-Path $GamesDir 'jup'
    if ($jup) {
        if ((Test-Path (Join-Path $jupDir 'bo2mp_pack.txt')) -and -not $RedoJup) {
            Note 'jup: the small copy is already in the export (-RedoJup makes it again into a new folder).'
        } else {
            if (Test-Path $jupDir) { $jupDir = Join-Path $GamesDir ('jup-' + (Get-Date -Format 'yyyyMMdd-HHmm')) ; Warn "games\jup is not empty: the new small copy goes to $jupDir; rename it to jup when it is good." }
            Say "Modern Warfare III small copy (CodLegacy.exe export-mw3) from $($jup.Game.Source)"
            Write-Config $Destination $false
            $psi = @{ CODLEGACY_GAME_PATHS = $GamePaths; CODLEGACY_NO_DETECT = '1'; CODLEGACY_WINDOW_HIDDEN = '1' }
            $out = Join-Path $Destination 'export-mw3.log'
            $code = Invoke-CodLegacy $ExeDest @('export-mw3', $jupDir) $psi $out ([Math]::Max($VerifyMinutes, 90)) $null
            if ($code -ne 0 -or -not (Test-Path (Join-Path $jupDir 'bo2mp_pack.txt'))) { Warn "The Modern Warfare III small copy failed (exit $code, see $out): Modern Warfare III/II guns are not in the export." }
            else { Note "jup: small copy made ($(Gb (Get-ChildItem -LiteralPath $jupDir -Recurse -File | Measure-Object Length -Sum).Sum))" }
        }
    }
}

if (-not $VerifyOnly) {
    Say 'Settings, music, profile'
    $mw3src = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'MW3_export'
    if (Test-Path -LiteralPath (Join-Path $mw3src 'manifest.json')) { Robo $mw3src (Join-Path $Destination 'MW3_export') @() @('/E') }
    Write-Config $Destination $false
    if (Test-Path -LiteralPath (Join-Path $PlayDir 'music.cfg')) {
        Copy-Item -LiteralPath (Join-Path $PlayDir 'music.cfg') -Destination $Destination -Force
        $abs = @(Get-Content -LiteralPath (Join-Path $PlayDir 'music.cfg') | Where-Object { $_ -notmatch '^\s*(//|#)' -and $_ -match '[A-Za-z]:[\\/]' })
        if ($abs.Count) { Warn "music.cfg names files outside the export (not copied): $($abs -join ' | ')" }
    }
    if (Test-Path -LiteralPath (Join-Path $PlayDir 'music')) { Robo (Join-Path $PlayDir 'music') (Join-Path $Destination 'music') @() @('/E') }
    if (Test-Path -LiteralPath (Join-Path $PlayDir 'banners')) { Robo (Join-Path $PlayDir 'banners') (Join-Path $Destination 'banners') @() @('/E') }
    # His own files (diag::profile_dir): `profile` beside the play folder; the
    # export runs from its own folder, so they go to <export>\profile.
    $prof = if ((Split-Path $PlayDir -Leaf) -ieq 'play') { Join-Path (Split-Path $PlayDir) 'profile' } else { Join-Path $PlayDir 'profile' }
    if (Test-Path -LiteralPath $prof) { Robo $prof (Join-Path $Destination 'profile') @() @('/E') }
    $art = Join-Path $PlayDir 'codlegacy-artifacts'
    if (Test-Path -LiteralPath $art) {
        $x = @('/E', '/XD', 'logs', 'screenshots', 'clips', 'dumps', 'demos', 'traces', 'captures')
        if (-not $IncludeCache) { $x += 'cache' }
        Robo $art (Join-Path $Destination 'codlegacy-artifacts') @() $x
    }
    [System.IO.File]::WriteAllText((Join-Path $GamesDir 'call_of_claude.txt'), "Call of Claude: the files the BO2 rebuild reads, copied out of each game by tools/export/export-portable.ps1.`r`n")
    $bat = "@echo off`r`nrem Starts the BO2 rebuild from this folder, with settings from this folder only.`r`ncd /d `"%~dp0`"`r`n"
    foreach ($k in $PathKeys) { $bat += "set `"$k=`"`r`n" }
    $bat += "start `"`" CodLegacy.exe frontend t6mp`r`n"
    [System.IO.File]::WriteAllText((Join-Path $Destination 'Play BO2 Rebuild.bat'), $bat)
    $readme = @"
BO2 Rebuild - portable folder (personal copy)

Start: double-click "Play BO2 Rebuild.bat" (or CodLegacy.exe frontend t6mp).
Everything the game reads is in this folder: games\<game> holds trimmed copies
of the original games' files, game_paths.cfg and .env point only inside it
(relative paths: the folder can be moved or copied to another drive whole).

DO NOT SHARE THIS FOLDER. It contains files copied from your own game
installs (Black Ops II, Black Ops, Modern Warfare 2 and 3, World at War,
Call of Duty 4, Ghosts, Advanced Warfare, Infinite Warfare, Black Ops III,
Modern Warfare Remastered, Modern Warfare III). They are for your own use on
your own PC only; uploading or giving them to anyone is not allowed.

Left out on purpose: the Modern Warfare Remastered, Advanced Warfare,
Modern Warfare III (2023) and Modern Warfare II (2022) maps, and the Black
Ops III Zombies maps and campaign. Their guns are kept.

export-report.txt says what was copied and what the last check found.
Made by tools/export/export-portable.ps1 on $(Get-Date -Format 'yyyy-MM-dd HH:mm').
"@
    [System.IO.File]::WriteAllText((Join-Path $Destination 'README.txt'), $readme)
}

# --- verify --------------------------------------------------------------------
function Path-Violations([string[]]$files) {
    $allowed = @($Destination, $env:windir, $env:SystemRoot, $env:TEMP, $env:TMP, $env:ProgramData, (Join-Path $env:ProgramFiles 'WindowsApps'),
        (Join-Path $env:ProgramFiles 'NVIDIA Corporation'), (Join-Path $env:ProgramFiles 'AMD'), (Join-Path $env:USERPROFILE '.cargo'), (Join-Path $env:USERPROFILE '.rustup')) | Where-Object { $_ } | ForEach-Object { $_.Replace('/', '\').TrimEnd('\') + '\' }
    $bad = New-Object System.Collections.ArrayList
    foreach ($f in $files) {
        if (-not (Test-Path -LiteralPath $f)) { continue }
        foreach ($line in [System.IO.File]::ReadLines($f)) {
            foreach ($m in [regex]::Matches($line, '(?i)(?<![A-Za-z])[A-Z]:[\\/][^"''<>|*?\r\n]*')) {
                $p = $m.Value.Replace('/', '\')
                if (-not ($allowed | Where-Object { $p.StartsWith($_, 'OrdinalIgnoreCase') -or ($p + '\') -ieq $_ })) { $bad.Add("$([System.IO.Path]::GetFileName($f)): $($line.Trim())") | Out-Null; break }
            }
        }
    }
    return $bad
}
function Gun-Counts([string]$file) {
    $c = @{}
    if (Test-Path -LiteralPath $file) {
        foreach ($m in [regex]::Matches((Get-Content -Raw -LiteralPath $file), '(?m)^\s*(\w+): (\d+) guest guns OK, (\d+) with gaps')) { $c[$m.Groups[1].Value] = [int]$m.Groups[2].Value }
    }
    return $c
}
function Latest-Log([string]$dir) {
    Get-ChildItem -LiteralPath (Join-Path $dir 'codlegacy-artifacts\logs') -Filter *.log -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1 | ForEach-Object FullName
}

$verifyFailed = $false
if (-not $SkipVerify) {
    Say 'Verify: the export alone (no original game folder reachable)'
    if (-not (Test-Path -LiteralPath $ExeDest)) { throw "No CodLegacy.exe in $Destination" }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $vroot = Join-Path $Destination "verify\$stamp"
    # 1. Every folder the export's settings name is inside it.
    $static = New-Object System.Collections.ArrayList
    foreach ($kv in (Read-KeyFile (Join-Path $Destination 'game_paths.cfg')).GetEnumerator()) {
        if (-not $kv.Value) { continue }
        $full = if ([System.IO.Path]::IsPathRooted($kv.Value)) { [System.IO.Path]::GetFullPath($kv.Value) } else { [System.IO.Path]::GetFullPath((Join-Path $Destination $kv.Value)) }
        if (-not $full.StartsWith($Destination + '\', 'OrdinalIgnoreCase')) { $static.Add("game_paths.cfg $($kv.Key)=$($kv.Value)") | Out-Null }
    }
    foreach ($kv in (Read-KeyFile (Join-Path $Destination '.env')).GetEnumerator()) {
        if ($kv.Value -match '^[A-Za-z]:[\\/]' -and -not ([System.IO.Path]::GetFullPath($kv.Value)).StartsWith($Destination + '\', 'OrdinalIgnoreCase')) { $static.Add(".env $($kv.Key)=$($kv.Value)") | Out-Null }
    }
    if ($static.Count) { $verifyFailed = $true; Warn ("Settings name folders outside the export: " + ($static -join '; ')) } else { Note 'verify: game_paths.cfg and .env name only folders inside the export' }

    # 2. A copy of the game in a fresh folder (empty cache, so every gun is
    # read from the export's files), absolute settings into the export, the
    # home folder pointed at an empty folder and every game setting cleared.
    $iso = Join-Path $vroot 'isolated'
    New-Item -ItemType Directory -Force -Path $iso | Out-Null
    Copy-Item -LiteralPath $ExeDest -Destination $iso -Force
    Write-Config $iso $true
    $isoOut = Join-Path $vroot 'check-guns-export.txt'
    Write-Host "  check-guns on the export (up to $VerifyMinutes minutes) ..."
    $code = Invoke-CodLegacy (Join-Path $iso 'CodLegacy.exe') @('check-guns') @{ CODLEGACY_NO_DETECT = '1'; CODLEGACY_BO3_WORKSHOP = '0'; CODLEGACY_WINDOW_HIDDEN = '1'; BO2MP_REPO_DIR = (Join-Path $vroot 'none') } $isoOut $VerifyMinutes (Join-Path $vroot 'home')
    if ($code -ne 0) { $verifyFailed = $true; Warn "check-guns on the export exited $code (see $isoOut)" }
    $logs = @($isoOut, "$isoOut.err", (Latest-Log $iso))

    # 3. The export's own folder as he will start it (relative settings).
    $relOut = Join-Path $vroot 'check-guns-export-folder.txt'
    $code2 = Invoke-CodLegacy $ExeDest @('check-guns') @{ CODLEGACY_WINDOW_HIDDEN = '1' } $relOut $VerifyMinutes $null
    if ($code2 -ne 0) { $verifyFailed = $true; Warn "check-guns from the export folder exited $code2 (see $relOut)" }
    $logs += @($relOut, "$relOut.err", (Latest-Log $Destination))

    # 4. Maps, one per game kept (optional: each opens the game hidden).
    if ($VerifyMaps) {
        foreach ($k in $Keys) {
            $zone = Prop $Rules.verify_maps $k
            if (-not $zone -or -not $Exported.Contains($k)) { continue }
            $stem = ($zone -split ':')[-1]
            if (-not (Get-ChildItem -LiteralPath (Join-Path $GamesDir $k) -Recurse -File -Filter "$stem.ff" -ErrorAction SilentlyContinue | Select-Object -First 1)) { Note "verify map: $k $zone not in the export, skipped"; continue }
            $mo = Join-Path $vroot "map-$k.txt"
            Write-Host "  map $zone ..."
            $mc = Invoke-CodLegacy (Join-Path $iso 'CodLegacy.exe') @('map', $zone, '--cmds', 'quit') @{ CODLEGACY_NO_DETECT = '1'; CODLEGACY_BO3_WORKSHOP = '0'; CODLEGACY_WINDOW_HIDDEN = '1' } $mo 10 (Join-Path $vroot 'home')
            $ml = Latest-Log $iso
            if ($ml) { Copy-Item -LiteralPath $ml -Destination (Join-Path $vroot "map-$k.log") -Force }
            $logs += @($mo, "$mo.err", (Join-Path $vroot "map-$k.log"))
            if ($mc -ne 0) { $verifyFailed = $true; Warn "map $zone from the export exited $mc (see $mo)" } else { Note "verify map: $k $zone loaded and quit" }
        }
    }

    # 5. Nothing outside the export named in what the runs printed or logged.
    $bad = @(Path-Violations ($logs | Where-Object { $_ }))
    if ($bad.Count) {
        $verifyFailed = $true
        Warn "the export runs named $($bad.Count) paths outside the export:"
        $bad | Select-Object -First 30 | ForEach-Object { Write-Host "    $_" -ForegroundColor Red; $script:Report.Add("  outside: $_") | Out-Null }
    } else { Note 'verify: no path outside the export in the runs'' output and logs' }

    # 6. The same guns as with the original folders.
    $base = Join-Path $Destination 'verify\baseline'
    New-Item -ItemType Directory -Force -Path $base | Out-Null
    Copy-Item -LiteralPath $ExeDest -Destination $base -Force
    $baseOut = Join-Path $vroot 'check-guns-originals.txt'
    Write-Host '  check-guns on the original folders (for comparison) ...'
    $benv = @{ CODLEGACY_GAME_PATHS = $GamePaths; CODLEGACY_NO_DETECT = '1'; CODLEGACY_WINDOW_HIDDEN = '1' }
    if ($SourceEnv['CODLEGACY_GAMES']) { $benv['CODLEGACY_GAMES'] = $SourceEnv['CODLEGACY_GAMES'] }
    if ($SourceEnv['CODLEGACY_T6_MOD']) { $benv['CODLEGACY_T6_MOD'] = $SourceEnv['CODLEGACY_T6_MOD'] }
    $null = Invoke-CodLegacy (Join-Path $base 'CodLegacy.exe') @('check-guns') $benv $baseOut $VerifyMinutes $null
    $was = Gun-Counts $baseOut; $now = Gun-Counts $isoOut
    Note ''
    Note ('{0,-6} {1,12} {2,12}' -f 'guns', 'originals', 'export')
    foreach ($g in (@($was.Keys) + @($now.Keys) | Sort-Object -Unique)) {
        $a = if ($was.ContainsKey($g)) { $was[$g] } else { 0 }; $b = if ($now.ContainsKey($g)) { $now[$g] } else { 0 }
        Note ('{0,-6} {1,12} {2,12}{3}' -f $g, $a, $b, $(if ($b -lt $a) { '   FEWER IN THE EXPORT' } else { '' }))
        if ($b -lt $a) { $verifyFailed = $true }
    }
    if (-not $now.Count) { $verifyFailed = $true; Warn "check-guns on the export printed no gun counts (see $isoOut)" }
    Note "verify files: $vroot (delete old verify folders by hand when done)"
}

# --- report ------------------------------------------------------------------
$size = (Get-ChildItem -LiteralPath $Destination -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notlike "$Destination\verify\*" } | Measure-Object Length -Sum).Sum
Note "Export size: $(Gb $size) in $Destination"
if ($script:Warnings.Count) { Note ''; Note 'Warnings:'; $script:Warnings | ForEach-Object { Note "  $_" } }
$head = "BO2 Rebuild export report, $(Get-Date -Format 'yyyy-MM-dd HH:mm')`r`nSources: $GamePaths`r`n"
$verdict = if ($SkipVerify) { 'NOT VERIFIED (-SkipVerify)' } elseif ($verifyFailed) { 'VERIFY FAILED' } else { 'VERIFIED: runs from the export alone' }
[System.IO.File]::AppendAllText((Join-Path $Destination 'export-report.txt'), $head + ($script:Report -join "`r`n") + "`r`nResult: $verdict`r`n`r`n")
if ($verifyFailed) { Write-Host "`n$verdict - see $Destination\export-report.txt" -ForegroundColor Red; exit 1 }
Write-Host "`n$verdict - $Destination" -ForegroundColor Green
