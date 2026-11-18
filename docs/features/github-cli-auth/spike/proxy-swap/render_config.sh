# Writes $STATE/config/config.yaml (JSON body) with the GitHub binding and an echo binding.
# Usage: render_config.sh [github|nogithub]
#   github   — the GitHub secret entry is present (the real token is swapped in)
#   nogithub — the entry is left out, so the placeholder reaches GitHub unswapped
. "$(dirname "$0")/lib.sh"
MODE="${1:-github}"
mkdir -p "$STATE/config"
GH_HOSTS='[{"host":"github.com"},{"host":"api.github.com"},{"host":"uploads.github.com"}]'
# Echo binding: a fake "real" value, so the swap mechanics are visible in an echo service's reply.
ECHO_ENTRY='{
  "source": {"type": "file", "path": "/etc/iron-proxy/secrets/ECHO_REAL"},
  "rules": [{"host":"httpbin.org"}],
  "replace": {"proxy_value": "'"$PLACEHOLDER"'", "match_headers": ["Authorization"]}
}'
GH_ENTRY='{
  "source": {"type": "file", "path": "/etc/iron-proxy/secrets/GH_TOKEN"},
  "rules": '"$GH_HOSTS"',
  "replace": {"proxy_value": "'"$PLACEHOLDER"'", "match_headers": ["Authorization"]}
}'
if [ "$MODE" = github ]; then SECRETS="[$ECHO_ENTRY, $GH_ENTRY]"; else SECRETS="[$ECHO_ENTRY]"; fi
cat > "$STATE/config/config.yaml.tmp" <<EOF
{
  "dns": { "listen": ":53", "proxy_ip": "$PROXY_IP" },
  "proxy": {
    "http_listen": ":80",
    "https_listen": ":443",
    "tunnel_listen": ":8080",
    "upstream_deny_cidrs": [
      "0.0.0.0/8", "10.0.0.0/8", "100.64.0.0/10", "127.0.0.0/8", "169.254.0.0/16",
      "172.16.0.0/12", "192.168.0.0/16", "::1/128", "fc00::/7", "fe80::/10", "fd00:ec2::254/128"
    ]
  },
  "tls": { "ca_cert": "/etc/iron-proxy/certs/ca.crt", "ca_key": "/etc/iron-proxy/certs/ca.key" },
  "transforms": [
    { "name": "allowlist", "config": { "warn": false, "domains": [
      "github.com", "api.github.com", "uploads.github.com", "codeload.github.com",
      "*.githubusercontent.com", "httpbin.org"
    ] } },
    { "name": "secrets", "config": { "secrets": $SECRETS } }
  ],
  "metrics": { "listen": "127.0.0.1:9090" },
  "management": { "listen": "127.0.0.1:9092" },
  "log": { "level": "info" }
}
EOF
mv -f "$STATE/config/config.yaml.tmp" "$STATE/config/config.yaml"
