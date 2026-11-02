// focus-space.js N | next | prev  (Windows port of the herdr kit's focus-space.sh)
// N: focus the Nth top-level space in sidebar order; worktree spaces nested under a repo are
// skipped, so N counts only the spaces you made. next/prev: step to the next or previous
// top-level space without wrapping; prev from a nested worktree space goes to its repo space.
// Pass --dry-run as a second argument to print the target instead of switching.
const { spawnSync } = require("child_process");
const herdr = process.env.HERDR_BIN_PATH || "herdr";
const arg = process.argv[2];
const dry = process.argv[3] === "--dry-run";

function run(args) {
  const out = spawnSync(herdr, args, { encoding: "utf8", windowsHide: true });
  return out.stdout || "";
}

try {
  const all = JSON.parse(run(["workspace", "list"])).result.workspaces
    .slice()
    .sort((a, b) => a.number - b.number);
  const linked = (w) => !!(w.worktree && w.worktree.is_linked_worktree);
  const tops = all.filter((w) => !linked(w));
  let target = null;
  if (arg === "next" || arg === "prev") {
    const cur = all.find((w) => w.focused);
    if (cur) {
      let i = -1;
      tops.forEach((w, k) => { if (w.number <= cur.number) i = k; });
      if (i >= 0) {
        if (arg === "next") target = tops[i + 1];
        else if (linked(cur)) target = tops[i];
        else if (i > 0) target = tops[i - 1];
      }
    }
  } else {
    target = tops[Number(arg) - 1];
  }
  if (target) {
    if (dry) console.log(target.workspace_id);
    else run(["workspace", "focus", target.workspace_id]);
  }
} catch {}
