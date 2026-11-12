# Builds the UI and packs the sources for the server: deploy\senate-src.tar.gz
# The archive carries a build stamp (commit, local edits, date), so the running node can say what it is.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
Push-Location $root
$info = Join-Path $root 'crates\senate-node\BUILD_INFO'
try {
  npm --prefix ui run build
  $commit = (git rev-parse --short HEAD).Trim()
  $dirty = [bool](git status --porcelain)
  $stamp = $commit + $(if ($dirty) { '+edits' } else { '' }) + ' ' + (Get-Date -Format 'yyyy-MM-dd HH:mm')
  [IO.File]::WriteAllText($info, $stamp, (New-Object Text.UTF8Encoding $false))
  $out = Join-Path $PSScriptRoot 'senate-src.tar.gz'
  if (Test-Path $out) { Remove-Item $out }
  tar -czf deploy/senate-src.tar.gz Cargo.toml Cargo.lock crates ui/dist deploy/Dockerfile deploy/docker-compose.yml deploy/install.sh deploy/receive.sh
  $mb = [math]::Round((Get-Item $out).Length / 1MB, 2)
  Write-Host "Done: $out ($mb MB), build $stamp"
  Write-Host "Next: deploy\push.ps1 (with the deploy key), or by hand:"
  Write-Host "  scp deploy\senate-src.tar.gz deploy\install.sh root@SERVER:/tmp/"
  Write-Host "  ssh root@SERVER bash /tmp/install.sh /tmp/senate-src.tar.gz"
} finally {
  if (Test-Path $info) { Remove-Item $info }
  Pop-Location
}
