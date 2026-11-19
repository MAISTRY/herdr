// Ctrl+T in Herdr: open a new tab in the focused pane's folder and start Claude in it.
// Answers the "trust this folder" prompt with Yes (the cursor starts on "No, exit").
// HERDR_TAB_NO_FOCUS=1 opens it in the background (used for testing).
const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const EXE = path.join(require("os").homedir(), ".herdr", "custom", "herdr.exe");
const HERDR = fs.existsSync(EXE) ? EXE : "herdr";
const LOG = path.join(__dirname, "new-claude-tab.log");
const log = (m) => fs.appendFileSync(LOG, `${new Date().toISOString()} ${m}\n`);
const herdr = (...a) => {
  try {
    return JSON.parse(execFileSync(HERDR, a, { encoding: "utf8", windowsHide: true, timeout: 60000 }));
  } catch (e) {
    const out = (e.stdout || "").toString();
    try { return JSON.parse(out); } catch { return { error: { message: out || e.message } }; }
  }
};
const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
const screen = (pane) => {
  try {
    return execFileSync(HERDR, ["pane", "read", pane, "--source", "recent-unwrapped", "--lines", "25"], {
      encoding: "utf8", windowsHide: true, timeout: 15000,
    });
  } catch {
    return "";
  }
};

function main() {
  const panes = herdr("pane", "list").result;
  const list = (panes && (panes.panes || panes)) || [];
  const here = list.find((p) => p.focused) || {};
  const cwd = here.cwd || process.env.USERPROFILE;
  const args = ["tab", "create", "--cwd", cwd, "--label", "claude"];
  if (here.workspace_id) args.push("--workspace", here.workspace_id);
  args.push(process.env.HERDR_TAB_NO_FOCUS ? "--no-focus" : "--focus");
  const tab = herdr(...args);
  const pane = tab.result && tab.result.root_pane && tab.result.root_pane.pane_id;
  if (!pane) return log(`tab create failed: ${JSON.stringify(tab.error || tab)}`);

  const base = path.basename(cwd.replace(/[\\/]+$/, "")).toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^[^a-z]+/, "");
  const name = `${(base || "claude").slice(0, 26)}-${Date.now().toString(36).slice(-4)}`;
  const extra = process.argv.slice(2); // e.g. -r for the resume picker
  herdr("agent", "start", name, "--kind", "claude", "--pane", pane, "--", "--model", "opus", "--effort", "high", ...extra);

  for (let i = 0; i < 20; i++) {
    const text = screen(pane);
    if (/trust this folder/i.test(text)) {
      if (/❯\s*Yes, I trust this folder/.test(text)) {
        herdr("pane", "send-keys", pane, "Enter");
        log(`${name} ${pane} trusted ${cwd}`);
      } else {
        herdr("pane", "send-keys", pane, "down");
      }
      sleep(800);
      continue;
    }
    if (/auto mode|for shortcuts|Try "|Resume Session|to search/i.test(text)) return log(`${name} ${pane} ready in ${cwd}`);
    sleep(1000);
  }
  log(`${name} ${pane} not confirmed ready in ${cwd}`);
}

main();
