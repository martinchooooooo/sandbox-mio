# Spike 3: exercises every audit-row shape once and saves the raw `podman logs --timestamps` lines
# to fixtures/podman-logs-timestamps.txt, the source for Phase E's translator tests.
# Needs a running topology from setup.sh and $STATE/../secrets.json from the secrets step.
. "$(dirname "$0")/lib.sh"
cd "$(dirname "$0")"
SECRETS="$(cat "$STATE/../secrets.json")"
apply() { sh render_config.sh "$1" "$2" "$3" && mgmt reload >/dev/null; }
since=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)

apply true '["example.com","httpbin.org"]' "$SECRETS"
sbx 'curl -s --noproxy "*" -o /dev/null https://example.com/'                      # DNS route, allowed
sbx 'curl -s --noproxy "*" -o /dev/null http://example.com/'                       # port-80 route
sbx 'curl -s -o /dev/null https://example.com/'                                    # tunnel: CONNECT + inner row
sbx 'curl -s --noproxy "*" -o /dev/null https://www.wikipedia.org/'                # warn annotation
apply false '["example.com","httpbin.org","*.nip.io","github.com"]' "$SECRETS"
sbx 'curl -s -o /dev/null https://www.wikipedia.org/'                              # CONNECT rejected
sbx 'curl -s --noproxy "*" -o /dev/null https://www.wikipedia.org/'                # request rejected
sbx 'curl -s -o /dev/null -H "Authorization: Bearer $OPENAI_API_KEY" https://httpbin.org/anything/h'
sbx 'curl -s -o /dev/null "https://httpbin.org/anything/q?key=$OPENAI_API_KEY"'
sbx 'curl -s -o /dev/null https://httpbin.org/anything/inject-me'                  # inject
sbx 'curl -s -o /dev/null http://10.88.0.1.nip.io/'                                # upstream_deny_cidrs error
sbx 'timeout 8 ssh -o StrictHostKeyChecking=no -o BatchMode=yes -o "ProxyCommand=socat - PROXY:10.90.250.2:%h:%p,proxyport=8080" -T git@github.com >/dev/null 2>&1'
apply false '["httpbin.org"]' "$(echo "$SECRETS" | sed 's#secrets/INJECTED#secrets/MISSING#')"
sbx 'curl -s -o /dev/null https://httpbin.org/anything/inject-me'                  # secret_unavailable
sed 's/"log":/"logg":/' "$STATE/config/config.yaml" > "$STATE/config/config.yaml.tmp" \
  && mv -f "$STATE/config/config.yaml.tmp" "$STATE/config/config.yaml" && mgmt reload >/dev/null   # 422
apply false '["example.com","httpbin.org"]' "$SECRETS"
mkdir -p fixtures
$P logs --timestamps --since "$since" "$SIDECAR" > fixtures/podman-logs-timestamps.txt 2>&1
wc -l fixtures/podman-logs-timestamps.txt
