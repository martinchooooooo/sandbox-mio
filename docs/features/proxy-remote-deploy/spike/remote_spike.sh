# Phase A spike of docs/features/proxy-remote-deploy/02_dev.md against SPIKE_CONNECTION (default mio-tests-remote).
# Needs a rootless Podman API socket on the target and a CA with keyUsage keyCertSign in $WORK/certs.
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'
P="podman --connection ${SPIKE_CONNECTION:-mio-tests-remote}"
WORK="${WORK:?directory holding config/config.yaml and certs/}"
N=spike-prd-int; S=spike-prd-proxy; X=spike-prd-sbx; PIP=10.90.251.2
$P network create --internal --disable-dns --subnet 10.90.251.0/24 $N
$P create --name $S --network $N:ip=$PIP --network podman --log-driver k8s-file --log-opt max-size=1mb \
  --env IRON_MANAGEMENT_API_KEY=tok --tmpfs /etc/iron-proxy/secrets:rw,mode=0755,size=1m \
  docker.io/ironsh/iron-proxy:0.56.0 -config /etc/iron-proxy/config/config.yaml
# /etc/iron-proxy does not exist in the created container: copying a staged tree onto it creates the path.
mkdir -p "$WORK/stage" && cp -r "$WORK/config" "$WORK/certs" "$WORK/stage/"
$P cp "$WORK/stage" $S:/etc/iron-proxy
$P start $S
printf sekrit | $P exec -i $S sh -c 'cat > /etc/iron-proxy/secrets/.K.tmp && mv /etc/iron-proxy/secrets/.K.tmp /etc/iron-proxy/secrets/K'
$P cp "$WORK/config/config.yaml" $S:/etc/iron-proxy/config/config.next.yaml
$P exec $S sh -c 'cd /etc/iron-proxy/config && mv config.yaml config.prev.yaml && mv config.next.yaml config.yaml && wget -q -S -O - --header "Authorization: Bearer tok" --post-data= http://127.0.0.1:9092/v1/reload'
$P run -d --name $X --network $N --dns $PIP --env http_proxy=http://$PIP:8080 localhost/sandbox-mio/base:10 sleep 600
$P exec $X sh -c "curl -s -m 8 -o /dev/null -w '%{http_code}\n' http://example.com; curl -s -m 5 --noproxy '*' https://1.1.1.1; echo rc=\$?"
$P logs --timestamps --since 2m $S
for c in $X $S; do $P rm -f $c; done; $P network rm $N
