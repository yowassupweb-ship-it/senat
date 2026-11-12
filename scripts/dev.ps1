# Dev loop: Vite (hot reload for the UI) + the desktop app pointed at it.
# UI edits appear instantly. Rust edits: stop the app window and run this again (or use `cargo watch -x run`).
$root = Split-Path $PSScriptRoot
$ui = Join-Path $root 'ui'
$vite = Start-Process -PassThru -WindowStyle Minimized -FilePath 'cmd.exe' -ArgumentList '/c', "cd /d `"$ui`" && npm run dev -- --host 127.0.0.1"
Start-Sleep 3
$env:SENATE_UI_DEV = 'http://127.0.0.1:5173'
try {
  Push-Location (Join-Path $root 'apps\desktop\src-tauri')
  cargo run
} finally {
  Pop-Location
  if ($vite -and -not $vite.HasExited) { Stop-Process -Id $vite.Id -Force }
}
