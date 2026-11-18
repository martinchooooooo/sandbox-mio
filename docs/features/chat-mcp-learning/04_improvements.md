---
type: improvements
title: "MCP in the Native Chat: Deferred Improvements"
feature: chat-mcp-learning
---
# MCP in the Native Chat: Deferred Improvements

These ideas were scoped out of `docs/features/chat-mcp-learning/01_ideas.md`. Each is worthwhile, but none is needed for the first build of the demo server and the chat's MCP visibility.

## An onboarding chapter on MCP

The illustrated onboarding book (`src/lib/onboarding/chapters/`) could gain a chapter explaining MCP as giving the agent more arms, ending with "switch on the demo server and ask the chat to roll dice". Deferred until the demo server and the tools popover exist to point at.

## A guided lesson mode

A scripted sequence in the chat would walk the learner through the demo tools in turn: call a tool, read the result, switch a server off, and watch the tool disappear. Deferred because the first build should show whether the empty-state prompt and the tools popover already teach enough.

## MCP resources and prompts in the demo

MCP servers can offer resources and prompts as well as tools, and pi exposes resource tools (`list_mcp_resources`, `read_mcp_resource`). Adding a resource to the demo server would teach the second MCP primitive. Deferred to keep the first demo to tools only.

## A protocol view

A collapsible view of the raw JSON-RPC request and response behind each MCP tool call would teach the protocol itself. Deferred because pi's RPC stream carries the tool call and result but not the JSON-RPC frames, so the view would need tracing inside the demo server.
