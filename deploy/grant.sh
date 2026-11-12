#!/usr/bin/env bash
# Run once on the server by deploy/grant-deploy-key.ps1. Installs the receiver and the deploy key,
# bound to that receiver alone: the key cannot open a shell, forward ports or run anything else.
# Revoke: delete the line ending in " senate-deploy" from /root/.ssh/authorized_keys.
set -euo pipefail
mkdir -p /opt/senate /root/.ssh
install -m 700 /tmp/receive.sh /opt/senate/receive.sh && sed -i 's/\r$//' /opt/senate/receive.sh
if [ -f /tmp/install.sh ]; then install -m 700 /tmp/install.sh /opt/senate/install.sh && sed -i 's/\r$//' /opt/senate/install.sh; fi
pub="$(tr -d '\r\n' < /tmp/senate_deploy.pub)"
touch /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys
sed -i '/ senate-deploy$/d' /root/.ssh/authorized_keys
echo "command=\"/opt/senate/receive.sh\",restrict ${pub% *} senate-deploy" >> /root/.ssh/authorized_keys
rm -f /tmp/receive.sh /tmp/install.sh /tmp/senate_deploy.pub /tmp/grant.sh
echo "Ключ выкладки установлен"
