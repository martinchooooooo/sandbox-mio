// Spike: derive unanswered extension dialogs from the supervisor's journal and input log.
// usage: node pending.mjs --config <ssh_config> --alias <alias>
import { execFileSync } from "node:child_process";

const argv = process.argv.slice(2);
const opt = {};
for (let i = 0; i < argv.length; i += 2) opt[argv[i].replace(/^--/, "")] = argv[i + 1];
const SSH = process.env.SPIKE_SSH ?? "C:\\Windows\\System32\\OpenSSH\\ssh.exe";
const fetch = (file) =>
  execFileSync(SSH, ["-F", opt.config, "-T", opt.alias, `cat /run/mio/pi-host/${file} 2>/dev/null || true`], { maxBuffer: 1 << 28 });
const lines = (buf) => buf.toString("utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));

const DIALOGS = new Set(["select", "confirm", "input", "editor"]);
const requests = lines(fetch("journal.jsonl")).filter((r) => r.type === "extension_ui_request" && DIALOGS.has(r.method));
const answered = new Set(lines(fetch("input.jsonl")).filter((r) => r.type === "extension_ui_response").map((r) => r.id));
console.log(JSON.stringify({ dialogs: requests.length, answered: answered.size, pending: requests.filter((r) => !answered.has(r.id)) }));
