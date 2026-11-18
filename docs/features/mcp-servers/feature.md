---
type: feature
title: "mcp-servers"
description: "An app-wide library of MCP servers, added by pasting a URL or README snippet and switched on per sandbox, rendered into the native MCP config of Claude Code, OpenCode and pi so Paseo, the terminal and the native chat all use them."
status: idea
architecture: [docs/architecture/agents.md, docs/architecture/network.md, docs/architecture/workspace-ui.md]
---

# mcp-servers

An app-wide library of MCP servers, added by pasting a URL or README snippet and switched on per sandbox, rendered into the native MCP config of Claude Code, OpenCode and pi so Paseo, the terminal and the native chat all use them.

## Status note

Ideation drafted. The gating spike is partial (2026-11-19, no Podman): Claude Code checks pass on the host; OpenCode, pi and the Paseo launch path remain to be tested in a sandbox before `02_dev.md`. See `spike/mcp-config-locations.md`.
