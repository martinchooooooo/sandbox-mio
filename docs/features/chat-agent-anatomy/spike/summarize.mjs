// Spike analyzer: collapses a recorded trace into its beats. Consecutive `message_update`
// deltas of one kind become one line with a count and time span.
// Usage: node summarize.mjs <trace.jsonl>
import fs from "node:fs";

const lines = fs.readFileSync(process.argv[2], "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const rows = [];
for (const { ms, record } of lines) {
    let label = record.type;
    if (record.type === "message_update") {
        const ev = record.assistantMessageEvent ?? {};
        label = `  update:${ev.type}`;
        if (ev.type === "toolcall_start") label += ` id=${ev.id ?? ev.toolCall?.id ?? "?"} tool=${ev.toolName ?? "?"}`;
        if (ev.type === "toolcall_end") label += ` id=${ev.toolCall?.id} ${ev.toolCall?.name} ${JSON.stringify(ev.toolCall?.arguments)}`;
    } else if (record.type === "message_start" || record.type === "message_end") {
        const m = record.message ?? {};
        label += ` role=${m.role}`;
        if (record.type === "message_end" && m.role === "assistant") {
            label += ` content=[${(m.content ?? []).map((c) => c.type).join(",")}] stop=${m.stopReason}`;
        }
        if (m.role === "toolResult") label += ` id=${m.toolCallId}`;
    } else if (record.type.startsWith("tool_execution")) {
        label += ` id=${record.toolCallId} ${record.toolName}`;
        if (record.type === "tool_execution_start") label += ` ${JSON.stringify(record.args)}`;
        if (record.type === "tool_execution_end") label += ` isError=${record.isError} durationMs=${record.durationMs}`;
    }
    const last = rows.at(-1);
    if (last && last.label === label && label.includes("_delta")) {
        last.count += 1;
        last.end = ms;
    } else {
        rows.push({ label, start: ms, end: ms, count: 1 });
    }
}
for (const r of rows) {
    const span = r.count > 1 ? ` x${r.count} (${r.start}-${r.end}ms)` : ` @${r.start}ms`;
    console.log(r.label + span);
}
