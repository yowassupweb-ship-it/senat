#!/usr/bin/env bash
# Senate node in its own Docker container. Run as root on the server:  bash install.sh /tmp/senate-src.tar.gz
# Touches nothing of MBOX: separate compose project (senate), network and volumes.
set -euo pipefail

SRC="${1:-/tmp/senate-src.tar.gz}"
DIR=/opt/senate
PORT="${SENATE_PORT:-17777}"

[ "$(id -u)" = 0 ] || { echo "Запустите от root"; exit 1; }
[ -f "$SRC" ] || { echo "Нет архива $SRC"; exit 1; }
docker compose version >/dev/null 2>&1 || { echo "Нужен docker compose (плагин v2)"; exit 1; }
if ss -ltn 2>/dev/null | grep -q "127.0.0.1:$PORT "; then
  if ! docker ps --format '{{.Names}}' | grep -qx senate-node; then
    echo "Порт $PORT на сервере уже занят не нодой Senate. Выберите другой: SENATE_PORT=18888 bash install.sh …"; exit 1
  fi
fi

mkdir -p "$DIR"
rm -rf "$DIR/src" && mkdir "$DIR/src"
tar -xzf "$SRC" -C "$DIR/src"
cd "$DIR/src/deploy"

# The disk is shared with MBOX and small. A build needs ~3 GB on the way; running out of it in the middle stops
# MBOX's database from writing. Make room first, and do not start a build that would fill the disk.
docker image prune -f >/dev/null 2>&1 || true
docker builder prune -f >/dev/null 2>&1 || true
free_mb=$(df --output=avail -B1M / | tail -1 | tr -d ' ')
if [ "${free_mb:-0}" -lt 3000 ]; then
  docker image inspect senate-node:prev >/dev/null 2>&1 && docker rmi senate-node:prev >/dev/null 2>&1 || true
  docker builder prune --all -f >/dev/null 2>&1 || true
  free_mb=$(df --output=avail -B1M / | tail -1 | tr -d ' ')
fi
if [ "${free_mb:-0}" -lt 3000 ]; then
  echo "На диске сервера свободно ${free_mb} МБ, сборке нужно не меньше 3000: выкладка не начата, чтобы не остановить базу MBOX. Освободите место."
  exit 1
fi

# keep the previous image for a quick rollback
docker image inspect senate-node:latest >/dev/null 2>&1 && docker tag senate-node:latest senate-node:prev || true

echo "Сборка образа (первый раз — несколько минут)…"
SENATE_PORT="$PORT" docker compose -p senate build
SENATE_PORT="$PORT" docker compose -p senate up -d

for i in $(seq 1 60); do
  st="$(docker inspect --format '{{.State.Health.Status}}' senate-node 2>/dev/null || echo none)"
  [ "$st" = healthy ] && ok=1 && break
  sleep 2
done
[ "${ok:-0}" = 1 ] || { echo "Нода не стала healthy. Лог: docker logs senate-node"; exit 1; }

# The disk is shared with MBOX and small: drop what this build left behind that nothing uses (untagged images,
# dangling build cache). Tagged images — senate-node:prev for the rollback among them — and volumes stay.
docker image prune -f >/dev/null 2>&1 || true
docker builder prune -f >/dev/null 2>&1 || true

TOKEN="$(docker exec senate-node cat /data/token)"
cat <<EOF

Senate node работает в контейнере senate-node (порт $PORT, только на самом сервере).
Токен доступа: $TOKEN

Подключиться с компьютера через SSH-туннель:
  ssh -N -L 7777:127.0.0.1:$PORT root@<адрес сервера>
  в десктопе:   senate-desktop --remote http://127.0.0.1:7777 --token $TOKEN
  в браузере:   http://127.0.0.1:7777/#token=$TOKEN

Вход в агентов: Настройки → Агенты (ссылка и код появятся прямо там).
Журнал:   docker logs -f senate-node
Остановка: cd $DIR/src/deploy && docker compose -p senate down        (данные сохранятся)
Откат:     docker tag senate-node:prev senate-node:latest && cd $DIR/src/deploy && docker compose -p senate up -d --no-build
EOF
