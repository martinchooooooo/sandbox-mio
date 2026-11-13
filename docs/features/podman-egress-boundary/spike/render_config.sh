# Writes $STATE/config/config.yaml as a JSON body (Resolved Decision 12).
# Usage: render_config.sh <warn true|false> <domains json array> [secrets json array]
. "$(dirname "$0")/lib.sh"
WARN="$1"; DOMAINS="$2"; SECRETS="${3:-[]}"
mkdir -p "$STATE/config"
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
  "tls": { "ca_cert": "/etc/iron-proxy/certs/ca.pem", "ca_key": "/etc/iron-proxy/certs/ca-key.pem" },
  "transforms": [
    { "name": "allowlist", "config": { "warn": $WARN, "domains": $DOMAINS } },
    { "name": "secrets", "config": { "secrets": $SECRETS } }
  ],
  "metrics": { "listen": "127.0.0.1:9090" },
  "management": { "listen": "127.0.0.1:9092" },
  "log": { "level": "info" }
}
EOF
mv -f "$STATE/config/config.yaml.tmp" "$STATE/config/config.yaml"
