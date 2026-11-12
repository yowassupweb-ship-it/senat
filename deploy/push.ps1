# Updates the server node with the deploy key (see grant-deploy-key.ps1).
#   push.ps1            pack and deploy; refused while specialists are working
#   push.ps1 -Force     deploy even while they work (it interrupts them)
#   push.ps1 -Status    what runs on the server now
#   push.ps1 -Rollback  back to the previous image
param([switch]$Force, [switch]$Status, [switch]$Rollback, [switch]$NoPack)
$ErrorActionPreference = 'Stop'
$cfg = (Get-Content "$env:APPDATA\Senate\remote.json" -Raw).TrimStart([char]0xFEFF) | ConvertFrom-Json
$server = $cfg.ssh.host
$key = Join-Path $env:USERPROFILE '.ssh\senate_deploy'
if (-not (Test-Path $key)) { throw 'No deploy key yet: run deploy\grant-deploy-key.ps1 once' }
$ssh = "ssh -i `"$key`" -o BatchMode=yes -o ConnectTimeout=15 root@$server"
if ($Status) { cmd /c "$ssh status"; exit $LASTEXITCODE }
if ($Rollback) { cmd /c "$ssh rollback"; exit $LASTEXITCODE }
if (-not $NoPack) { & "$PSScriptRoot\pack.ps1" }
$archive = Join-Path $PSScriptRoot 'senate-src.tar.gz'
$what = if ($Force) { '"deploy force"' } else { 'deploy' }
# cmd redirection: PowerShell 5.1 would re-encode a binary stream piped to a native program
cmd /c "$ssh $what < `"$archive`""
exit $LASTEXITCODE
