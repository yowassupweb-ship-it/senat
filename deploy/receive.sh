#!/usr/bin/env bash
# The only thing the deploy key may run (forced command in /root/.ssh/authorized_keys).
#   deploy        - read a senate-src.tar.gz from stdin and install it (refuses while specialists are working)
#   deploy force  - the same, even while they are working
#   status        - what runs now: build stamp, health, live work
#   rollback      - go back to the previous image
# The node token never leaves the server: every line of output is scrubbed of it.
set -uo pipefail
DIR=/opt/senate
PORT="${SENATE_PORT:-17777}"
CMD="${SSH_ORIGINAL_COMMAND:-status}"

token() { docker exec senate-node cat /data/token 2>/dev/null || true; }
TOKEN="$(token)"
scrub() { if [ -n "$TOKEN" ]; then sed -u "s/$TOKEN/[токен скрыт]/g"; else cat; fi | grep -v -e '#token=' -e '--token '; }
api() { curl -fsS --max-time 5 -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:$PORT$1" 2>/dev/null; }
live_count() { api /api/live | grep -o '"session_id"' | wc -l; }

case "$CMD" in
  status)
    echo "health: $(curl -fsS --max-time 5 http://127.0.0.1:$PORT/api/health 2>/dev/null || echo 'нет ответа')"
    echo "контейнер: $(docker inspect --format '{{.State.Status}} с {{.State.StartedAt}}' senate-node 2>/dev/null || echo 'нет')"
    echo "идёт работ: $(live_count)"
    ;;
  rollback)
    exec 9>"$DIR/deploy.lock"; flock -n 9 || { echo "Сейчас идёт другая выкладка"; exit 1; }
    docker image inspect senate-node:prev >/dev/null 2>&1 || { echo "Предыдущего образа нет"; exit 1; }
    docker tag senate-node:prev senate-node:latest && cd "$DIR/src/deploy" && SENATE_PORT="$PORT" docker compose -p senate up -d --no-build 2>&1 | scrub
    echo "Откат выполнен"
    ;;
  deploy|"deploy force")
    mkdir -p "$DIR"
    exec 9>"$DIR/deploy.lock"
    flock -n 9 || { echo "Сейчас идёт другая выкладка, попробуйте через пару минут"; exit 1; }
    n="$(live_count)"
    if [ "$CMD" = deploy ] && [ "${n:-0}" -gt 0 ]; then
      echo "Специалисты сейчас работают ($n). Выкладка перезапустит ноду и оборвёт их. Повторите позже или с 'deploy force'."
      exit 2
    fi
    f="$(mktemp /tmp/senate-src.XXXXXX.tar.gz)"
    trap 'rm -f "$f"' EXIT
    head -c 200000000 > "$f"
    tar -tzf "$f" >/dev/null 2>&1 || { echo "Архив повреждён"; exit 1; }
    tar -xzf "$f" -O deploy/install.sh > "$DIR/install.sh.new" 2>/dev/null && [ -s "$DIR/install.sh.new" ] && mv "$DIR/install.sh.new" "$DIR/install.sh" || rm -f "$DIR/install.sh.new"
    SENATE_PORT="$PORT" bash "$DIR/install.sh" "$f" 2>&1 | scrub
    rc="${PIPESTATUS[0]}"
    echo "health: $(curl -fsS --max-time 5 http://127.0.0.1:$PORT/api/health 2>/dev/null || echo 'нет ответа')"
    exit "$rc"
    ;;
  *)
    echo "Этот ключ умеет только: deploy, deploy force, status, rollback"
    exit 1
    ;;
esac
