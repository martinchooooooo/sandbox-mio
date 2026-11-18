# Phase 2: real GitHub through the sidecar, with only the placeholder inside the sandbox.
# Needs $STATE/secrets/GH_TOKEN (the real token, no trailing newline). Never prints the token.
# Writes nothing to GitHub: the push check is `git push --dry-run`.
. "$(dirname "$0")/lib.sh"
TOKEN_FILE="$STATE/secrets/GH_TOKEN"
[ -s "$TOKEN_FILE" ] || { echo "missing $TOKEN_FILE" >&2; exit 1; }
# Counts lines containing the real token in stdin, without echoing either.
leaks() { grep -c -F -f "$TOKEN_FILE" || true; }

echo "== G0 reload with the GitHub binding"
sh "$(dirname "$0")/render_config.sh" github && mgmt reload | grep -m1 'HTTP/'

echo "== G1 gh auth login --with-token <placeholder>"
sbx "rm -rf /root/.config/gh; printf '%s' '$PLACEHOLDER' | gh auth login --hostname github.com --git-protocol https --with-token; echo exit=\$?"
sbx "gh auth setup-git --hostname github.com; echo exit=\$?"

echo "== G2 what the sandbox stores"
sbx "grep -c '$PLACEHOLDER' /root/.config/gh/hosts.yml"
echo -n "real token in hosts.yml: "; $P exec "$SANDBOX" cat /root/.config/gh/hosts.yml | leaks
echo -n "gh auth token prints placeholder: "; sbx "gh auth token" | grep -c "$PLACEHOLDER"

echo "== G3 gh API calls"
sbx "gh auth status 2>&1 | sed -n '1,4p'"
sbx "gh api user --jq .login"

echo "== G4 git read (private repo if any)"
REPO=$(sbx "gh repo list --visibility private --limit 1 --json nameWithOwner --jq '.[0].nameWithOwner'")
[ -n "$REPO" ] || REPO=$(sbx "gh repo list --limit 1 --json nameWithOwner --jq '.[0].nameWithOwner'")
echo "repo: $REPO"
sbx "git ls-remote https://github.com/$REPO HEAD; echo exit=\$?"

echo "== G5 git push --dry-run (authenticates to receive-pack, writes nothing)"
sbx "rm -rf /tmp/ghs && git clone -q --depth 1 --filter=blob:none --no-checkout https://github.com/$REPO /tmp/ghs && cd /tmp/ghs && git push --dry-run origin HEAD:refs/heads/spike-ghs-dryrun 2>&1; echo exit=\$?"

echo "== G6 leak checks"
echo -n "real token in sidecar log lines: "; $P logs "$SIDECAR" 2>&1 | leaks
echo -n "real token in sandbox git config: "; $P exec "$SANDBOX" sh -c 'cat /root/.gitconfig /tmp/ghs/.git/config 2>/dev/null' | leaks
echo "audit swap annotations for GitHub hosts:"
$P logs "$SIDECAR" 2>&1 | grep '"msg":"request"' | grep -E 'github\.com' | grep -o '"host":"[^"]*"\|"swapped":\[[^]]*\]' | paste - - | sort | uniq -c | head

echo "== G7 negative: binding removed, placeholder reaches GitHub unswapped"
sh "$(dirname "$0")/render_config.sh" nogithub && mgmt reload | grep -m1 'HTTP/'
sbx "gh api user --jq .login 2>&1 | head -2"
sbx "git ls-remote https://github.com/$REPO HEAD 2>&1 | head -2"

echo "== G8 restore the binding"
sh "$(dirname "$0")/render_config.sh" github && mgmt reload | grep -m1 'HTTP/'
sbx "gh api user --jq .login"
