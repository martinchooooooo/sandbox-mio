---
type: improvements
title: "MCP Servers: Deferred Improvements"
feature: mcp-servers
---
# MCP Servers: Deferred Improvements

These ideas were scoped out of `docs/features/mcp-servers/01_ideas.md`. Each is worthwhile, but none is needed for the first build of the MCP library and its per-sandbox switch.

## Proxy placeholder swapping for MCP secrets

In a proxied sandbox, the secret file could hold a placeholder instead of the real value. The proxy sidecar would swap in the real value only on requests to the server's host (`docs/architecture/network.md`, Secret swapping), so the agent would never see the real value. Deferred because the proxy is optional and swapping only covers HTTP to bound hosts. Local stdio servers that call non-HTTP services would still need the real value.

## MCP tool policy in the iron-proxy sidecar

iron-proxy can filter `tools/list` and refuse `tools/call` through a default-deny tool allowlist (`docs/features/podman-egress-boundary/04_improvements.md`). The policy would give a user per-tool control over a remote server, for proxied Podman sandboxes only. Deferred because a non-technical user first needs servers working at all.

## More import formats

The import parser could also accept a `claude mcp add …` command line, VS Code's `servers` key and Codex's TOML `mcp_servers` tables. Deferred because the `mcpServers` JSON shape covers most README instructions, and each extra format needs its own parser and tests.

## A curated catalogue of popular servers

The library page could offer one-click entries for well-known servers, with plain-language descriptions and the secret fields already named. Deferred because a catalogue needs upkeep as servers change their URLs and auth, and pasting already covers any server a user is told about.

## Deployed sandboxes

The reconcile skips deployed sandboxes, as the GitHub reconcile does (`docs/features/github-cli-auth/02_dev.md`, Resolved Decision 4). Supporting a deployed sandbox means writing secret files on the remote host, which the app does not do for any credential today.

## An in-sandbox MCP aggregator

One in-sandbox process could front every switched-on server, so each agent needs a single config entry. It would also give the app one place to read connection health. Deferred because the aggregator is a long-running process and a third-party dependency to supervise, while per-agent rendering needs neither.

## Other agents

Codex and any later catalogue agent need their own row in the rendering table. Deferred until the catalogue (`src/lib/agents/catalogue.ts`) installs such an agent.
