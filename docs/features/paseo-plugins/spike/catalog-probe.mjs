// Connects to a Paseo daemon as a browser client and prints the plugin catalog's client bundles.
// Usage (inside the sandbox): node catalog-probe.mjs <ws-url> <password>
// Run with NODE_PATH pointing at the pinned CLI's node_modules so @getpaseo/client resolves.
import { createRequire } from "node:module";

const require = createRequire(process.env.PASEO_CLIENT_DIR + "/");
const { DaemonClient } = await import(require.resolve("@getpaseo/client/internal/daemon-client"));
const [url, password] = process.argv.slice(2);
const client = new DaemonClient({ url, password, clientId: "mio-spike", clientType: "browser" });
await client.connect();
const plugins = await client.getPluginCatalog();
for (const p of plugins) {
  const bundle = p.clientBundle ?? "";
  console.log(JSON.stringify({
    id: p.id ?? p.pluginId,
    keys: Object.keys(p),
    clientBundleBytes: bundle.length,
    mentionsProbeTitle: /Sandbox-mio probe v\d/.exec(bundle)?.[0] ?? null,
  }));
}
process.exit(0);
