# One-time setup: lets deploy\push.ps1 update the server node without the root password.
# Asks for the root password twice (copy, then install). The new key can only run deploy\receive.sh.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$cfg = Get-Content "$env:APPDATA\Senate\remote.json" -Raw
$cfg = $cfg.TrimStart([char]0xFEFF) | ConvertFrom-Json
$server = if ($args.Count -gt 0) { $args[0] } else { $cfg.ssh.host }
if (-not $server) { throw 'No server: pass it as the first argument' }
$key = Join-Path $env:USERPROFILE '.ssh\senate_deploy'
if (-not (Test-Path $key)) {
  & ssh-keygen -q -t ed25519 -C senate-deploy -N '""' -f $key
  if ($LASTEXITCODE -ne 0) { throw 'ssh-keygen failed' }
}
Copy-Item "$key.pub" "$env:TEMP\senate_deploy.pub" -Force
& scp -o StrictHostKeyChecking=accept-new "$here\receive.sh" "$here\install.sh" "$here\grant.sh" "$env:TEMP\senate_deploy.pub" "root@${server}:/tmp/"
if ($LASTEXITCODE -ne 0) { throw 'copy failed' }
& ssh "root@$server" "sed -i 's/\r$//' /tmp/grant.sh && bash /tmp/grant.sh"
if ($LASTEXITCODE -ne 0) { throw 'install failed' }
Remove-Item "$env:TEMP\senate_deploy.pub" -ErrorAction SilentlyContinue
& ssh -i $key -o BatchMode=yes "root@$server" status
Write-Host 'Done. Update the server with: deploy\push.ps1'
