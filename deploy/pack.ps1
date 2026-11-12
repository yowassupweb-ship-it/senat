# Builds the UI and packs the sources for the server: deploy\senate-src.tar.gz
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
Push-Location $root
try {
  npm --prefix ui run build
  $out = Join-Path $PSScriptRoot 'senate-src.tar.gz'
  if (Test-Path $out) { Remove-Item $out }
  tar -czf deploy/senate-src.tar.gz Cargo.toml Cargo.lock crates ui/dist deploy/Dockerfile deploy/docker-compose.yml
  $mb = [math]::Round((Get-Item $out).Length / 1MB, 2)
  Write-Host "Done: $out ($mb MB)"
  Write-Host "Next, in cmd:"
  Write-Host "  scp deploy\senate-src.tar.gz deploy\install.sh root@SERVER:/tmp/"
  Write-Host "  ssh root@SERVER bash /tmp/install.sh /tmp/senate-src.tar.gz"
} finally { Pop-Location }
