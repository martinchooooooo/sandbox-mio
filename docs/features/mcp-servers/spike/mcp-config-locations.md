---
type: spike
title: "Spike: MCP Config Locations and Secret References (Partial, Host-Side)"
feature: mcp-servers
---
# Spike: MCP Config Locations and Secret References (Partial, Host-Side)

Run on 2026-11-19. The gating spike was meant to run in one real Podman sandbox with all three agents installed. That did not happen: the Podman machine was stopped, `podman machine start` did not return within several minutes, and `wsl -d podman-machine-default` then hung with dozens of stuck `wsl.exe` processes left over, so no container could be started. The agents were not available in a sandbox either. As a fallback, the Claude Code checks ran on the Windows host against a throwaway home directory (`HOME` and `USERPROFILE` pointed at a temp directory, so no real `~/.claude.json` was read or written). Every secret was the dummy value `hunter2`. No container was started and nothing needed cleanup beyond temp directories.

## Environment

- **Host Claude Code:** 2.1.327 on Windows 11. Host `node` and Git Bash `sh` stand in for the sandbox's mise-provided node and `/bin/sh`.
- **Host pi:** 0.79.4. It predates built-in MCP (0.99.0), so no pi check was possible; the sandbox install is pinned at 1.5.0.
- **OpenCode:** not installed on the host.
- **Paseo:** the `paseo` CLI exists on the host, but no Paseo daemon was driven; no launch-path check ran.

## Results

| # | Check | Result | Evidence |
|---|---|---|---|
| 1a | Claude Code reads top-level `mcpServers` of `$HOME/.claude.json` (user scope), stdio server | PASS (host) | `claude mcp list` reported the trivial server `Connected` (`mini-mcp.mjs`) |
| 1b | OpenCode reads `opencode.json` `mcp` from `/root/.config/opencode/` | NOT TESTED | No sandbox, no host install |
| 1c | pi reads `mcp.json` from `PI_CODING_AGENT_DIR` and connects in `--mode rpc` | NOT TESTED | Host pi 0.79.4 has no MCP; no sandbox with pi 1.5.0 |
| 2a | Claude Code `headersHelper` supplies headers from a secret file | PASS (host) | A streamable-HTTP server answering only with the right header saw it on every request and `Connected`; with no helper the same server returned 401 and Claude reported a failed connection (`mini-mcp-http.mjs`, `headers-helper.sh`) |
| 2b | Claude Code stdio `env` with `${VAR}` | PASS (host) | `${MYV}` in `env` reached the server as the value of `MYV` from the launching process environment |
| 2c | Launcher script reads a secret file into an env var, then execs the server | PASS (host) | The server saw `hunter2` read from the file (`launcher.sh`) |
| 2d | OpenCode `{file:…}` substitution | NOT TESTED | OpenCode unavailable |
| 2e | pi whole-value `!command` reading the secret file | NOT TESTED | pi 1.5.0 unavailable |
| 3 | An agent started from Paseo sees the servers | NOT TESTED | No sandbox, no driven Paseo daemon |

## Findings

- **`${VAR}` in Claude Code `env` expands from Claude's own process environment, not from a file.** The design row "`${VAR}` for stdio `env` through a launcher" only works if something puts the variable into the environment of the process that starts Claude Code. Paseo, the terminal and the chat start Claude differently (the rejected "environment variables on each agent process" option in `01_ideas.md`). The launcher form is the viable one: the config's `command` is a small script that reads the secret file, exports the variable and execs the real server, so the config holds only a file path. It worked on the host.
- **`headersHelper` takes a command line and runs it per connection.** On Windows the helper had to be invoked as `sh.exe script secret-path`; in the Linux sandbox a plain script path works.
- **Claude Code retried and probed with GET and POST requests on a remote server.** A secret-requiring server sees several requests per `mcp list`; the helper therefore runs several times, so it must be cheap and side-effect free.

## Remaining Work

The spike is not closed. Before `02_dev.md`, repeat checks 1b, 1c, 2d, 2e and 3, and re-run 1a, 2a to 2c inside the Linux sandbox, once the Podman machine starts. The probe files here are reusable as they stand: the sandbox needs only node (through mise) and `sh`.

## Files

- `mini-mcp.mjs`: stdio MCP server with one tool, which reports the length of `DUMMY_SECRET`.
- `mini-mcp-http.mjs`: streamable-HTTP MCP server that requires header `X-Dummy: hunter2`.
- `headers-helper.sh`: prints a headers JSON object from a secret file, for `headersHelper`.
- `launcher.sh`: exports a secret file's content as an env var, then execs the server.
