#!/usr/bin/env bash
# The Unix bridge and the Zenoh bridge (arora-bridge-zenoh) through every path a client takes to a device in a container:
# the same container, a shared volume (another container, the host), ssh -L from another container, ssh -J through a
# relay container, ssh -L from the host; and, for the Unix bridge, relay.py over ssh's standard streams, through an
# OpenSSH before 6.7 (which forwards no socket file) running python 2.7, as on a NAO, from a container, through a
# relay and from the host. `check` (functional, against the Arora runtime) on every path, then `bench`
# (against the stand-in) on every path. Output: $OUT/checks.log, $OUT/bench.jsonl, $OUT/device_rtt.jsonl.
#
# Build the binary first (cargo build --release -p arora-bridge-unix-bench); the image builds from this directory's
# Dockerfile, its BASE (BASE=…) an image whose glibc is at least the build machine's; the old sshd's from
# Dockerfile.old-ssh (OLD_BASE=…, ubuntu:14.04 by default). Rootless Docker or root.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
B=${B:-$HERE/../../../target/release/bridge-bench}
S=$(mktemp -d)
OUT=${OUT:-$S/results}
N=${N:-3000}
PUSH_S=${PUSH_S:-5}
IMG=arora-bridge-unix-ssh:test
OLD_IMG=arora-bridge-unix-old-ssh:test
docker build -q -t $IMG --build-arg BASE="${BASE:-ubuntu:24.04}" "$HERE" >/dev/null
docker build -q -t $OLD_IMG --build-arg BASE="${OLD_BASE:-ubuntu:14.04}" -f "$HERE/Dockerfile.old-ssh" "$HERE" >/dev/null
echo "results in $OUT" >&2
# This run's containers and network are named after it, so it touches nothing it did not create; inside
# the network each answers to its short name (robot, relay, ws, nao).
RUN=arora-bt-$$
NET=$RUN
mkdir -p "$OUT" "$S/keys" "$S/shared"
[ -f "$S/keys/key" ] || ssh-keygen -q -t ed25519 -N '' -f "$S/keys/key"
cp "$S/keys/key.pub" "$S/keys/authorized_keys"
cat > "$S/keys/config" <<'EOF'
Host *
  User root
  IdentityFile /keys/key
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
  LogLevel ERROR
  StreamLocalBindUnlink yes
  ExitOnForwardFailure yes
  ServerAliveInterval 5
EOF

cleanup() { docker rm -f $RUN-robot $RUN-relay $RUN-ws $RUN-nao >/dev/null 2>&1 || true
            docker network rm $NET >/dev/null 2>&1 || true
            [ -n "${HOSTFWD:-}" ] && kill $HOSTFWD 2>/dev/null || true; }
trap cleanup EXIT
docker network create $NET >/dev/null

start() { # name, image, extra docker args...
  local name=$1 image=$2; shift 2
  docker run -d --init --rm --name "$RUN-$name" --hostname "$name" --network $NET --network-alias "$name" -v "$S/keys:/keys:ro" \
    -v "$B:/usr/local/bin/bridge-bench:ro" -v "$HERE/..:/relay:ro" "$@" "$image" sh -c '
      mkdir -p /root/.ssh /run/arora && cp /keys/authorized_keys /keys/config /root/.ssh/ && chmod 700 /root/.ssh &&
      chmod 600 /root/.ssh/* && cp /keys/key /root/.ssh/id_ed25519 && chmod 600 /root/.ssh/id_ed25519 &&
      ssh-keygen -A >/dev/null && exec /usr/sbin/sshd -D -e' >/dev/null
}
start robot $IMG -v "$S/shared:/run/arora" -p 127.0.0.1:2222:22
start relay $IMG
start ws $IMG -v "$S/shared:/mnt/robot"
# The robot's sshd: it sees the device's socket directory at the same path.
start nao $OLD_IMG -v "$S/shared:/run/arora" -p 127.0.0.1:2223:22
sleep 2
# An OpenSSH before 6.7 refuses to forward a socket file, the reason for relay.py: the client binds the local file,
# and a connection through it fails.
old_sshd_forwards() {
  docker exec -d "$RUN-ws" ssh -N -L /tmp/old.sock:/run/arora/dev.sock nao; sleep 2
  local forwarded=0
  timeout 20 docker exec "$RUN-ws" bridge-bench check --unix /tmp/old.sock >/dev/null 2>&1 && forwarded=1
  docker exec "$RUN-ws" sh -c 'pkill -x ssh; rm -f /tmp/old.sock' || true
  [ $forwarded = 1 ]
}
# The workstation container's key is /keys/key via its config; the host uses the same key file.
HOSTSSH=(ssh -i "$S/keys/key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR
         -o StreamLocalBindUnlink=yes -o ExitOnForwardFailure=yes)

serve() { # mode, transport args...
  docker exec "$RUN-robot" pkill -f 'bridge-bench serve' 2>/dev/null || true; sleep 0.5
  local mode=$1; shift
  docker exec -d "$RUN-robot" sh -c "bridge-bench serve --mode $mode $* > /run/arora/serve.log 2>&1"
  sleep 1.5
}
forwards() { # (re)open the ssh forwards in ws and on the host
  docker exec "$RUN-ws" sh -c 'pkill -x ssh; rm -f /tmp/*.sock;
    ssh -f -N -L /tmp/l.sock:/run/arora/dev.sock -L /tmp/lz.sock:/run/arora/z.sock -L 17447:127.0.0.1:7447 robot &&
    ssh -f -N -J relay -L /tmp/j.sock:/run/arora/dev.sock -L /tmp/jz.sock:/run/arora/z.sock -L 27447:127.0.0.1:7447 robot'
  [ -n "${HOSTFWD:-}" ] && kill $HOSTFWD 2>/dev/null || true
  rm -f "$S/host-l.sock" "$S/host-lz.sock"
  "${HOSTSSH[@]}" -N -L "$S/host-l.sock:/run/arora/dev.sock" -L "$S/host-lz.sock:/run/arora/z.sock" \
    -L 37447:127.0.0.1:7447 -p 2222 root@127.0.0.1 & HOSTFWD=$!
  sleep 1.5
}

# path label | where the client runs | its transport args, for the unix bridge, zenoh tcp, zenoh unixsock
run_paths() { # subcommand, server kind (unix|ztcp|zunix), extra args
  local sub=$1 kind=$2; shift 2
  local -a rows
  case $kind in
    unix)  rows=("same-container|robot|--unix /run/arora/dev.sock" "shared-volume:container|ws|--unix /mnt/robot/dev.sock"
                 "shared-volume:host|host|--unix $S/shared/dev.sock" "ssh-L:container|ws|--unix /tmp/l.sock"
                 "ssh-J-relay:container|ws|--unix /tmp/j.sock" "ssh-L:host|host|--unix $S/host-l.sock"
                 "ssh-stdio-py3:container|ws|--stdio=ssh robot python3 /relay/relay.py /run/arora/dev.sock"
                 "ssh-stdio-old-sshd-py27:container|ws|--stdio=ssh nao python /relay/relay.py /run/arora/dev.sock"
                 "ssh-stdio-old-sshd-py27-J-relay:container|ws|--stdio=ssh -J relay nao python /relay/relay.py /run/arora/dev.sock"
                 "ssh-stdio-old-sshd-py27:host|host|--stdio=${HOSTSSH[*]} -p 2223 root@127.0.0.1 python /relay/relay.py /run/arora/dev.sock") ;;
    ztcp)  rows=("same-container|robot|--zenoh tcp/127.0.0.1:7447" "docker-net:container|ws|--zenoh tcp/robot:7447"
                 "ssh-L:container|ws|--zenoh tcp/127.0.0.1:17447" "ssh-J-relay:container|ws|--zenoh tcp/127.0.0.1:27447"
                 "ssh-L:host|host|--zenoh tcp/127.0.0.1:37447") ;;
    zunix) rows=("same-container|robot|--zenoh unixsock-stream//run/arora/z.sock"
                 "shared-volume:container|ws|--zenoh unixsock-stream//mnt/robot/z.sock"
                 "shared-volume:host|host|--zenoh unixsock-stream/$S/shared/z.sock"
                 "ssh-L:container|ws|--zenoh unixsock-stream//tmp/lz.sock"
                 "ssh-J-relay:container|ws|--zenoh unixsock-stream//tmp/jz.sock"
                 "ssh-L:host|host|--zenoh unixsock-stream/$S/host-lz.sock") ;;
  esac
  for row in "${rows[@]}"; do
    IFS='|' read -r path where args <<<"$row"
    local label="$kind $path" extra=() targs
    [ "$sub" != check ] && extra=(--label "$label" "$@")
    # A --stdio=COMMAND is one argument; the others split on spaces.
    if [[ $args == --stdio=* ]]; then targs=("$args"); else read -ra targs <<<"$args"; fi
    echo "== $sub $label" >&2
    if [ "$where" = host ]; then
      timeout 300 "$B" $sub "${targs[@]}" "${extra[@]}" || echo "FAIL $label exited $?"
    else
      timeout 300 docker exec "$RUN-$where" bridge-bench $sub "${targs[@]}" "${extra[@]}" || echo "FAIL $label exited $?"
    fi
  done
}

for kind in unix ztcp zunix; do
  case $kind in unix) t="--unix /run/arora/dev.sock";; ztcp) t="--zenoh tcp/0.0.0.0:7447";;
                zunix) t="--zenoh unixsock-stream//run/arora/z.sock";; esac
  serve device "$t"; forwards
  if [ $kind = unix ]; then
    if old_sshd_forwards; then echo "FAIL the old sshd forwarded a socket file"; else echo "PASS the old sshd forwards no socket file"; fi \
      | tee -a "$OUT/checks.log"
    forwards
  fi
  run_paths check $kind 2>&1 | sed "s/^/[$kind] /" | tee -a "$OUT/checks.log"
  run_paths device-rtt $kind --n 300 >> "$OUT/device_rtt.jsonl"
  serve standin "$t"; forwards
  run_paths bench $kind --n "$N" --push-s "$PUSH_S" >> "$OUT/bench.jsonl"
done
grep -c '^\[.*PASS' "$OUT/checks.log" | sed 's/^/PASS lines: /'
if grep -q FAIL "$OUT/checks.log" "$OUT/bench.jsonl" "$OUT/device_rtt.jsonl"; then echo SOME FAILED; exit 1; fi
echo MATRIX OK
