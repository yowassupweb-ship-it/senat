$root = Split-Path $PSScriptRoot
$exe = Join-Path $root 'apps\desktop\src-tauri\target\release\senate-desktop.exe'
$desktop = [Environment]::GetFolderPath('Desktop')

if (-not (Test-Path $exe)) {
  throw "Senate is not built: $exe`nRun: cargo build --release --manifest-path apps/desktop/src-tauri/Cargo.toml"
}

$ws = New-Object -ComObject WScript.Shell
$start = $ws.CreateShortcut("$desktop\Senate.lnk")
$start.TargetPath = $exe
$start.Arguments = ''
$start.IconLocation = "$exe,0"
$start.WorkingDirectory = $root
$start.Description = 'Senate desktop'
$start.Save()

Get-ChildItem "$desktop\Senate*.lnk" | ForEach-Object {
  $shortcut = $ws.CreateShortcut($_.FullName)
  if ($shortcut.Arguments -match 'senate-stop\.ps1') { Remove-Item $_.FullName -Force }
}

Get-ChildItem "$desktop\Senate*.lnk" | Select-Object Name, FullName
