#!/bin/sh
set -eu
relay=0
installed=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --relay) relay=1 ;;
    --installed) installed=1 ;;
    *) echo 'usage: run_product.sh [--relay] [--installed]' >&2; exit 2 ;;
  esac
  shift
done
labdir=$(mktemp -d /tmp/veyra-product-tls.XXXXXXXX)
server_pid=''
cleanup() {
  if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi
  rm -rf "$labdir"
}
trap cleanup EXIT HUP INT TERM
umask 077
# Operator TLS keys are permitted configuration, distinct from user E2EE keys.
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$labdir/ca.key" -out "$labdir/ca.pem" -days 1 -subj '/CN=Veyra synthetic laboratory CA' -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
openssl req -new -newkey rsa:2048 -nodes -keyout "$labdir/server.key" -out "$labdir/server.csr" -subj '/CN=localhost' >/dev/null 2>&1
cat > "$labdir/server.ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost,IP:127.0.0.1
EOF
openssl x509 -req -in "$labdir/server.csr" -CA "$labdir/ca.pem" -CAkey "$labdir/ca.key" -CAcreateserial -out "$labdir/server.pem" -days 1 -extfile "$labdir/server.ext" >/dev/null 2>&1
if [ "$installed" -eq 1 ]; then
  # Compile only laboratory tools. Installed /usr/bin clients are never rebuilt.
  cargo build --locked -p veyra-signaling
  cargo test --locked -p veyra-daemon --test session_process --no-run --message-format=json > "$labdir/test-build.jsonl"
  test_binary=$(python3 - "$labdir/test-build.jsonl" <<'PYBIN'
import json,os,sys
paths=[]
with open(sys.argv[1],encoding='utf-8') as stream:
    for line in stream:
        entry=json.loads(line)
        if entry.get('reason')=='compiler-artifact' and entry.get('target',{}).get('name')=='session_process' and entry.get('executable'):
            paths.append(entry['executable'])
if len(paths)!=1 or not os.path.isabs(paths[0]):
    raise SystemExit('Could not identify the laboratory test executable')
print(paths[0])
PYBIN
  )
  daemon_bin=${VEYRA_TEST_DAEMON_BIN:-/usr/bin/veyra-daemon}
  vault_bin=${VEYRA_TEST_VAULT_BIN:-/usr/bin/veyra-vault}
  sha256sum "$daemon_bin" "$vault_bin" > "$labdir/installed-before.sha256"
  cat "$labdir/installed-before.sha256"
else
  cargo build --locked -p veyra-daemon -p veyra-signaling
fi
if [ "$relay" -eq 1 ]; then
  # PUBLIC synthetic operator secret, restricted to the isolated Coturn fixture.
  printf '%s' 'veyra-synthetic-rest-secret-only-0123456789abcdef' > "$labdir/turn.secret"
  printf '{"urls":["turn:coturn-rest-laboratory:3478?transport=udp"],"auth_secret_file":"%s"}\n' "$labdir/turn.secret" > "$labdir/issuer.json"
  target/debug/veyra-signaling '127.0.0.1:18443' "$labdir/server.pem" "$labdir/server.key" "$labdir/issuer.json" &
else
  target/debug/veyra-signaling '127.0.0.1:18443' "$labdir/server.pem" "$labdir/server.key" &
fi
server_pid=$!
python3 - <<'PYREADY'
import socket,time
for attempt in range(50):
    try:
        with socket.create_connection(('127.0.0.1',18443),timeout=0.2):
            break
    except OSError:
        time.sleep(0.1)
else:
    raise SystemExit('TLS laboratory server did not start')
PYREADY
# Public CA path and public endpoint only. No identity keys or credentials in env.
if [ "$installed" -eq 1 ]; then
  PATH='' SSL_CERT_FILE="$labdir/ca.pem" VEYRA_TEST_WSS_ENDPOINT='wss://localhost:18443/v1/rendezvous' VEYRA_TEST_RELAY="$relay" VEYRA_TEST_DAEMON_BIN="$daemon_bin" VEYRA_TEST_VAULT_BIN="$vault_bin" VEYRA_TEST_REQUIRE_EMPTY_PATH=1 "$test_binary" --ignored --nocapture --test-threads=1
  sha256sum "$daemon_bin" "$vault_bin" > "$labdir/installed-after.sha256"
  cmp "$labdir/installed-before.sha256" "$labdir/installed-after.sha256"
else
  SSL_CERT_FILE="$labdir/ca.pem" VEYRA_TEST_WSS_ENDPOINT='wss://localhost:18443/v1/rendezvous' VEYRA_TEST_RELAY="$relay" cargo test --locked -p veyra-daemon --test session_process -- --ignored --nocapture --test-threads=1
fi
