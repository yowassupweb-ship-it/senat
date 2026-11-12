# Starts Senate already connected to the server node: keeps an SSH tunnel to it alive (key login, restarts on
# drop), then opens the desktop app, which remembers the node (remote.json). Run from the desktop shortcut.
param([string]$Server = '185.113.8.152', [int]$Port = 17777)
$root = Split-Path $PSScriptRoot
$exe = Join-Path $root 'apps\desktop\src-tauri\target\release\senate-desktop.exe'
$key = Join-Path $env:USERPROFILE '.ssh\senate_node'

function Test-Port { (Test-NetConnection 127.0.0.1 -Port $Port -WarningAction SilentlyContinue -InformationLevel Quiet) }

if (-not (Test-Port)) {
  $loop = "while (`$true) { ssh -i '$key' -o IdentitiesOnly=yes -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ServerAliveInterval=30 -o ExitOnForwardFailure=yes -N -L ${Port}:127.0.0.1:${Port} root@$Server; Start-Sleep 5 }"
  Start-Process powershell -ArgumentList '-NoProfile', '-WindowStyle', 'Hidden', '-Command', $loop -WindowStyle Hidden
  for ($i = 0; $i -lt 20 -and -not (Test-Port); $i++) { Start-Sleep -Milliseconds 500 }
}
Start-Process -FilePath $exe -WorkingDirectory $root
