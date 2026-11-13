# Builds the bounded-sandbox topology of 02_dev.md, The Concrete Model:
# internal --disable-dns network, dual-homed iron-proxy sidecar, sandbox with --dns <sidecar>.
# Expects $STATE/certs/ca.pem and ca-key.pem (from gen_ca) and a rendered $STATE/config/config.yaml.
. "$(dirname "$0")/lib.sh"
set -e
$P network create --internal --disable-dns --subnet "$SUBNET" "$NET"
$P create --name "$SIDECAR" \
  --network "$NET:ip=$PROXY_IP" --network podman \
  --log-driver k8s-file --log-opt max-size=1mb \
  --env IRON_MANAGEMENT_API_KEY="$MGMT_TOKEN" \
  -v "$STATE/certs:/etc/iron-proxy/certs:ro" \
  -v "$STATE/config:/etc/iron-proxy/config:ro" \
  -v "$STATE/secrets:/etc/iron-proxy/secrets:ro" \
  "$IRON_IMAGE" -config /etc/iron-proxy/config/config.yaml
$P start "$SIDECAR"
proxy_url="http://$PROXY_IP:8080"
$P create --name "$SANDBOX" --tty --network "$NET" --dns "$PROXY_IP" \
  --env http_proxy="$proxy_url" --env https_proxy="$proxy_url" \
  --env HTTP_PROXY="$proxy_url" --env HTTPS_PROXY="$proxy_url" \
  --env no_proxy=localhost,127.0.0.1,::1 --env NO_PROXY=localhost,127.0.0.1,::1 \
  --env NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/mio-proxy.crt \
  --env REQUESTS_CA_BUNDLE=/usr/local/share/ca-certificates/mio-proxy.crt \
  --env PIP_CERT=/usr/local/share/ca-certificates/mio-proxy.crt \
  --env SSL_CERT_FILE=/usr/local/share/ca-certificates/mio-proxy.crt \
  --env CURL_CA_BUNDLE=/usr/local/share/ca-certificates/mio-proxy.crt \
  --env GIT_SSL_CAINFO=/usr/local/share/ca-certificates/mio-proxy.crt \
  --env OPENAI_API_KEY=mio-placeholder-openai_api_key \
  "$BASE_IMAGE" /bin/sh
$P start "$SANDBOX"
# The inject_ca transport (src-tauri/src/proxy/mod.rs).
$P exec -i "$SANDBOX" sh -c 'cat >/usr/local/share/ca-certificates/mio-proxy.crt && update-ca-certificates' \
  < "$STATE/certs/ca.pem"
