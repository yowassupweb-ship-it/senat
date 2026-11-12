# Moves a Senate database (specialists, chats, memory) from another computer onto the server node.
#
# On the OLD computer: close Senate, then copy the folder  %APPDATA%\Senate  (files senate.db, senate.db-wal,
# senate.db-shm) to this computer. Then run:
#   powershell -File scripts\import-memory.ps1 -Source D:\old-senate
#
# What it does: stops the node container, saves the current server database next to the data
# (/data/backup-<time>/), puts the old files in place, starts the node again. Nothing else is touched.
param(
  [Parameter(Mandatory = $true)][string]$Source,
  [string]$Server = '185.113.8.152'
)
$ErrorActionPreference = 'Stop'
$db = Join-Path $Source 'senate.db'
if (-not (Test-Path $db)) { throw "No senate.db in $Source" }
$key = Join-Path $env:USERPROFILE '.ssh\senate_node'
$ssh = @('-i', $key, '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes')
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'

# fold the write-ahead log into the main file on a copy, so one file carries everything
$tmp = Join-Path $env:TEMP "senate-import-$stamp"
New-Item -ItemType Directory $tmp | Out-Null
Copy-Item (Join-Path $Source 'senate.db*') $tmp
$py = Get-Command py -ErrorAction SilentlyContinue
if ($py) {
  & py -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('PRAGMA wal_checkpoint(TRUNCATE)'); c.close()" (Join-Path $tmp 'senate.db')
  Remove-Item (Join-Path $tmp 'senate.db-wal'), (Join-Path $tmp 'senate.db-shm') -ErrorAction SilentlyContinue
}
Write-Host ("Database to import: {0:N0} KB" -f ((Get-Item (Join-Path $tmp 'senate.db')).Length / 1KB))

scp @ssh (Join-Path $tmp 'senate.db') "root@${Server}:/tmp/senate-import.db"
$remote = @"
set -e
docker stop senate-node
docker run -d --name senate-import -v senate_data:/data --user 0 --entrypoint sleep senate-node:latest 600
docker exec senate-import sh -c 'mkdir -p /data/backup-$stamp && cp -a /data/senate.db* /data/backup-$stamp/ 2>/dev/null || true; rm -f /data/senate.db-wal /data/senate.db-shm'
docker cp /tmp/senate-import.db senate-import:/data/senate.db
docker exec senate-import chown 1001:1001 /data/senate.db
docker rm -f senate-import
rm -f /tmp/senate-import.db
docker start senate-node
echo imported
"@
$sh = Join-Path $tmp 'import.sh'
[IO.File]::WriteAllText($sh, $remote.Replace("`r", ''), (New-Object Text.UTF8Encoding($false)))
scp @ssh $sh "root@${Server}:/tmp/senate-import.sh"
ssh @ssh "root@$Server" 'bash /tmp/senate-import.sh; rm -f /tmp/senate-import.sh'
Remove-Item $tmp -Recurse -Force
Write-Host "Done. Restart Senate. The previous server data is in /data/backup-$stamp inside the senate_data volume."
