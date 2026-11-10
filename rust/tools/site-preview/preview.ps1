# Shows the CodLegacy website on this PC with no build and no account service.
# Pages load; sign-in, servers and the leaderboard are empty because there is no service behind them.
# For the real thing (Steam sign-in works) use tools\accounts\run-local.cmd instead.
param([int]$Port = 8080)
$web = Join-Path $PSScriptRoot "..\..\web"
if (-not (Test-Path (Join-Path $web "index.html"))) { $web = Join-Path $PSScriptRoot "web" }   # when run from the zip
$web = (Resolve-Path $web).Path
$types = @{ ".html"="text/html; charset=utf-8"; ".js"="text/javascript; charset=utf-8"; ".css"="text/css; charset=utf-8";
            ".json"="application/json"; ".jpg"="image/jpeg"; ".png"="image/png"; ".svg"="image/svg+xml"; ".ico"="image/x-icon" }
$empty = @{ "/servers"='{"servers":[]}'; "/leaderboard"='[]'; "/competitions"='{"competitions":[]}' }

$l = New-Object System.Net.HttpListener
$l.Prefixes.Add("http://localhost:$Port/")
$l.Start()
Write-Host "CodLegacy site preview: http://localhost:$Port   (Ctrl+C or close this window to stop)"
Start-Process "http://localhost:$Port"
while ($l.IsListening) {
  $c = $l.GetContext()
  $path = [Uri]::UnescapeDataString($c.Request.Url.AbsolutePath)
  $bytes = $null; $type = "text/html; charset=utf-8"; $status = 200
  if ($empty.ContainsKey($path)) { $bytes = [Text.Encoding]::UTF8.GetBytes($empty[$path]); $type = "application/json" }
  elseif ($path -eq "/me" -or $path -like "/overlay/*" -or $path -eq "/teams" -or $path -eq "/tickets") { $status = 401; $bytes = [Text.Encoding]::UTF8.GetBytes('{"error":"unauthorized"}'); $type = "application/json" }
  else {
    $rel = $path.TrimStart("/").Replace("/", "\")
    if ($rel -eq "") { $rel = "index.html" }
    $file = Join-Path $web $rel
    $full = [IO.Path]::GetFullPath($file)
    if ($full.StartsWith($web) -and (Test-Path $full -PathType Leaf)) {
      $bytes = [IO.File]::ReadAllBytes($full)
      $ext = [IO.Path]::GetExtension($full).ToLower()
      if ($types.ContainsKey($ext)) { $type = $types[$ext] }
    } elseif ([IO.Path]::GetExtension($path) -eq "") {
      $bytes = [IO.File]::ReadAllBytes((Join-Path $web "index.html"))   # site pages like /play
    } else { $status = 404; $bytes = [Text.Encoding]::UTF8.GetBytes("not found") }
  }
  $c.Response.StatusCode = $status
  $c.Response.ContentType = $type
  $c.Response.ContentLength64 = $bytes.Length
  $c.Response.OutputStream.Write($bytes, 0, $bytes.Length)
  $c.Response.Close()
}
