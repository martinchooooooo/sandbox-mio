# Spike 1, transport half: the exec-tunnelled SSH alias (docs/architecture/transport.md) still works
# on the internal network. -L stands in for terminal/agent-UI/port forwards, -R for the local-LLM
# reverse forward (a sandbox-loopback listener relaying to a host server).
. "$(dirname "$0")/lib.sh"
K="$STATE/../sshkey"
HOST_PORT=47811
node -e "require('http').createServer((q,r)=>r.end('host-inference-ok\n')).listen($HOST_PORT,'127.0.0.1')" &
NODE_PID=$!
$P exec -d "$SANDBOX" sh -c 'socat TCP-LISTEN:7000,bind=127.0.0.1,fork,reuseaddr SYSTEM:"echo in-sandbox-service-ok"'
sleep 1
ssh -i "$K" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
  -o "ProxyCommand=podman --connection podman-machine-default exec -i $SANDBOX mio-sshd" \
  -N -L 127.0.0.1:47812:127.0.0.1:7000 -R 127.0.0.1:11434:127.0.0.1:$HOST_PORT root@spike &
SSH_PID=$!
sleep 4
echo "== ssh exec (terminal stand-in)"
ssh -i "$K" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
  -o "ProxyCommand=podman --connection podman-machine-default exec -i $SANDBOX mio-sshd" root@spike 'echo ssh-exec-ok; hostname'
echo "== -L forward host -> sandbox loopback service"
curl -s -m 5 http://127.0.0.1:47812/ 2>&1 | head -1 || true
node -e "const s=require('net').connect(47812,'127.0.0.1');s.on('data',d=>{process.stdout.write(d);s.end()});s.on('error',e=>console.log('err',e.message))"
echo "== -R forward sandbox loopback -> host (inference path), proxy env set as in proxy_env"
sbx 'curl -s -m 5 http://127.0.0.1:11434/'
kill $SSH_PID $NODE_PID 2>/dev/null
