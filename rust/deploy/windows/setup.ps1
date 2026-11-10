<#
  CodLegacy account service + website on a Windows VPS.  Run in an elevated PowerShell:

    .\setup.ps1 -Domain codlegacy.example.com -Exe .\codlegacy-accounts.exe -WebDir .\web

  Build the program on any Windows PC with:  cargo build --release -p accounts
  (the file is target\release\codlegacy-accounts.exe) and copy it, the `web` folder and this script to the VPS.
  Point the domain's DNS A record at the VPS first, or the HTTPS certificate cannot be issued.

  What it does: copies the files to C:\CodLegacy, makes the secret keys, installs Caddy (HTTPS in front),
  runs both as start-at-boot tasks that restart when they stop, and opens ports 80 and 443.
  Re-running with -Update replaces the program and the website and keeps the keys and database.
#>
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$WebDir,
  [string]$Domain = "",
  [string]$Root = "C:\CodLegacy",
  [switch]$Update
)
$ErrorActionPreference = "Stop"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw "Run this in an elevated (Administrator) PowerShell."
}
if (-not (Test-Path $Exe)) { throw "Program not found: $Exe" }
if (-not (Test-Path (Join-Path $WebDir "index.html"))) { throw "WebDir must be the website folder (it has index.html): $WebDir" }
if (-not $Update -and -not $Domain) { throw "Pass -Domain (the address players will use)." }

$bin = Join-Path $Root "bin"; $data = Join-Path $Root "data"; $web = Join-Path $Root "web"; $logs = Join-Path $Root "logs"
foreach ($d in @($Root, $bin, $data, $logs)) { New-Item -ItemType Directory -Force -Path $d | Out-Null }

function Stop-Tasks {
  foreach ($t in @("CodLegacy Accounts", "CodLegacy Caddy")) {
    if (Get-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue) { Stop-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue }
  }
  Start-Sleep -Seconds 2
}

function Lock-Down($path) {
  # Only Administrators and SYSTEM may read it: it holds secrets.
  icacls $path /inheritance:r /grant:r "Administrators:(F)" "SYSTEM:(F)" | Out-Null
}

function Random-Hex($bytes) {
  $b = New-Object byte[] $bytes
  [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b)
  -join ($b | ForEach-Object { $_.ToString("x2") })
}

Stop-Tasks
Copy-Item -Force $Exe (Join-Path $bin "codlegacy-accounts.exe")
if (Test-Path $web) { Remove-Item -Recurse -Force $web }
Copy-Item -Recurse -Force $WebDir $web

if ($Update) {
  if (-not (Test-Path (Join-Path $Root "run-accounts.cmd"))) { throw "No earlier install found in $Root; run without -Update first." }
} else {
  $serverKey = Random-Hex 24
  $adminKey = Random-Hex 24

  # The service reads its settings from the environment; this wrapper sets them and keeps a log.
  $run = @"
@echo off
set ACCOUNTS_BIND=127.0.0.1:8080
set ACCOUNTS_PUBLIC_URL=https://$Domain
set ACCOUNTS_DB=$data\accounts.sqlite
set ACCOUNTS_STATIC_DIR=$web
set ACCOUNTS_TRUST_IP_HEADER=X-Real-IP
set ACCOUNTS_SERVER_KEYS=eu1:$serverKey
set ACCOUNTS_ADMIN_KEY=$adminKey
"$bin\codlegacy-accounts.exe" >> "$logs\accounts.log" 2>&1
"@
  $runPath = Join-Path $Root "run-accounts.cmd"
  Set-Content -Path $runPath -Value $run -Encoding ASCII
  Lock-Down $runPath

  $keys = Join-Path $Root "KEYS.txt"
  Set-Content -Path $keys -Encoding ASCII -Value @"
CodLegacy keys (keep private; also inside run-accounts.cmd)
Game server key  (eu1): $serverKey   -> give to the game server as X-Server-Key
Admin key             : $adminKey   -> X-Admin-Key for /admin/* routes
"@
  Lock-Down $keys

  # Caddy: HTTPS certificates and the real client address.
  $caddy = Join-Path $bin "caddy.exe"
  if (-not (Test-Path $caddy)) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing -Uri "https://caddyserver.com/api/download?os=windows&arch=amd64" -OutFile $caddy
  }
  Set-Content -Path (Join-Path $Root "Caddyfile") -Encoding ASCII -Value @"
$Domain {
	encode gzip
	reverse_proxy 127.0.0.1:8080 {
		header_up X-Real-IP {remote_host}
	}
}
"@

  $settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -StartWhenAvailable -AllowStartIfOnBatteries
  $principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest
  $trigger = New-ScheduledTaskTrigger -AtStartup
  Register-ScheduledTask -Force -TaskName "CodLegacy Accounts" -Trigger $trigger -Settings $settings -Principal $principal `
    -Action (New-ScheduledTaskAction -Execute "cmd.exe" -Argument "/c `"$runPath`"" -WorkingDirectory $Root) | Out-Null
  Register-ScheduledTask -Force -TaskName "CodLegacy Caddy" -Trigger $trigger -Settings $settings -Principal $principal `
    -Action (New-ScheduledTaskAction -Execute $caddy -Argument "run --config `"$Root\Caddyfile`" --adapter caddyfile" -WorkingDirectory $Root) | Out-Null

  New-NetFirewallRule -DisplayName "CodLegacy HTTP/HTTPS" -Direction Inbound -Protocol TCP -LocalPort 80,443 -Action Allow -ErrorAction SilentlyContinue | Out-Null
}

Start-ScheduledTask -TaskName "CodLegacy Accounts"
if (-not $Update) { Start-ScheduledTask -TaskName "CodLegacy Caddy" } else { Start-ScheduledTask -TaskName "CodLegacy Caddy" -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 4

try {
  $r = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:8080/leaderboard" -TimeoutSec 10
  Write-Host "Account service is up (HTTP $($r.StatusCode))."
} catch {
  Write-Warning "The service did not answer yet. Look at $logs\accounts.log"
}
if (-not $Update) {
  Write-Host ""
  Write-Host "Done. Keys are in $Root\KEYS.txt (copy them somewhere safe, then keep that file private)."
  Write-Host "Check https://$Domain once DNS points here. Back up $data\accounts.sqlite regularly."
}
