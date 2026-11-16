// Prints median / min / max for every numeric column of the files in results/.
import { readFileSync, readdirSync } from "node:fs";
const med = (a) => { const s = [...a].sort((x, y) => x - y); const n = s.length; return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2; };
const fmt = (a) => `${Math.round(med(a))} (${Math.round(Math.min(...a))}-${Math.round(Math.max(...a))})`;
for (const f of readdirSync("results").sort()) {
  if (f === "summary.txt") continue;
  const t = readFileSync(`results/${f}`, "utf8").trim();
  if (!t) continue;
  let rows;
  if (f.endsWith(".json")) rows = JSON.parse(t);
  else if (f.endsWith(".jsonl")) rows = t.split("\n").map((l) => JSON.parse(l));
  else rows = t.split("\n").map((v) => ({ ms: Number(v) }));
  const errs = rows.filter((r) => r.error || r.timeout);
  console.log(`## ${f}  n=${rows.length}${errs.length ? "  ERRORS=" + errs.length : ""}`);
  const keys = [...new Set(rows.flatMap((r) => Object.keys(r)))];
  const out = [];
  for (const k of keys) {
    const v = rows.map((r) => r[k]).filter((x) => typeof x === "number");
    if (v.length && k !== "assetCount" && k !== "loginStatus") out.push(`${k}=${fmt(v)}`);
    else if (v.length === 0) { const s = [...new Set(rows.map((r) => r[k]))]; if (s.length < 4) out.push(`${k}=${s.join("|")}`); }
  }
  console.log("  " + out.join("  "));
}
