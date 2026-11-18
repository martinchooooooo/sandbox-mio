# Builds the bounded-sandbox topology with an iron-proxy sidecar and a base:10 sandbox (gh baked in).
# Generates the CA with `iron-proxy generate-ca` and writes the echo binding's fake value.
# The real GitHub token is NOT written here: copy it to $STATE/secrets/GH_TOKEN (no trailing newline).
. "$(dirname "$0")/lib.sh"
set -e
mkdir -p "$STATE/certs" "$STATE/secrets"
if [ ! -f "$STATE/certs/ca.crt" ]; then
  $P run --rm -v "$STATE/certs:/out" "$IRON_IMAGE" generate-ca -alg ed25519 -outdir /out
fi
printf '%s' 'ECHO-REAL-VALUE-swapped-by-sidecar' > "$STATE/secrets/ECHO_REAL"
[ -f "$STATE/config/config.yaml" ] || sh "$(dirname "$0")/render_config.sh" github
$P network create --internal --disable-dns --subnet "$SUBNET" "$NET"
$P create --name "$SIDECAR" \
  --network "$NET:ip=$PROXY_IP" --network podman \
  --log-driver k8s-file --log-opt max-size=4mb \
  --env IRON_MANAGEMENT_API_KEY="$MGMT_TOKEN" \
  -v "$STATE/certs:/etc/iron-proxy/certs:ro" \
  -v "$STATE/config:/etc/iron-proxy/config:ro" \
  -v "$STATE/secrets:/etc/iron-proxy/secrets:ro" \
  "$IRON_IMAGE" -config /etc/iron-proxy/config/config.yaml
$P start "$SIDECAR"
proxy_url="http://$PROXY_IP:8080"
CRT=/usr/local/share/ca-certificates/mio-proxy.crt
$P create --name "$SANDBOX" --tty --network "$NET" --dns "$PROXY_IP" \
  --env http_proxy="$proxy_url" --env https_proxy="$proxy_url" \
  --env HTTP_PROXY="$proxy_url" --env HTTPS_PROXY="$proxy_url" \
  --env no_proxy=localhost,127.0.0.1,::1 --env NO_PROXY=localhost,127.0.0.1,::1 \
  --env SSL_CERT_FILE="$CRT" --env CURL_CA_BUNDLE="$CRT" --env GIT_SSL_CAINFO="$CRT" \
  --env GH_NO_UPDATE_NOTIFIER=1 \
  "$BASE_IMAGE" /bin/sh
$P start "$SANDBOX"
$P exec -i "$SANDBOX" sh -c "cat >$CRT && update-ca-certificates >/dev/null" < "$STATE/certs/ca.crt"
echo "setup done"
