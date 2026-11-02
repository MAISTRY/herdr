#!/usr/bin/env node
// herdr plugin: show every <repo>/.claude/worktrees/* git worktree as its own space nested under its repo's space.
// DRY_RUN=1 prints the actions instead of performing them.
const fs = require('fs');
const os = require('os');
const path = require('path');
const net = require('net');
const { spawnSync } = require('child_process');

const DRY = process.env.DRY_RUN === '1';
const HERDR = process.env.HERDR_BIN_PATH || 'herdr';
const SHELLS = new Set(['pwsh', 'powershell', 'cmd', 'bash', 'sh', 'zsh']);
const log = (...a) => { if (DRY) console.log(...a); };

const norm = (p) => String(p || '').replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();

function run(cmd, args) {
  return spawnSync(cmd, args, { encoding: 'utf8', windowsHide: true, maxBuffer: 64 << 20 });
}
function herdrJson(args) {
  try { return JSON.parse(run(HERDR, args).stdout); } catch (e) { return null; }
}
function herdrDo(args) {
  if (DRY) { console.log('WOULD RUN: herdr ' + args.join(' ')); return null; }
  return herdrJson(args);
}

function isGitTop(dir) {
  try { return fs.existsSync(path.join(dir, '.git')); } catch (e) { return false; }
}

function repoWorktrees(repo) {
  const wtRoot = norm(path.join(repo, '.claude', 'worktrees')) + '/';
  const r = run('git', ['-C', repo, 'worktree', 'list', '--porcelain']);
  const out = [];
  for (const line of String(r.stdout || '').split(/\r?\n/)) {
    if (!line.startsWith('worktree ')) continue;
    const p = line.slice(9).trim();
    if (norm(p).startsWith(wtRoot)) out.push(p);
  }
  return out;
}

function sendMove(ids) {
  return new Promise((resolve) => {
    const sock = process.env.HERDR_SOCKET_PATH;
    if (!sock) return resolve(null);
    let buf = '';
    let c;
    let t;
    const done = (v) => { clearTimeout(t); try { c.destroy(); } catch (e) {} resolve(v); };
    try { c = net.connect(sock); } catch (e) { return resolve(null); }
    t = setTimeout(() => done(null), 5000);
    c.on('connect', () => c.write(JSON.stringify({ id: 'claude-worktrees:move', method: 'workspace.move_block', params: { workspace_ids: ids } }) + '\n'));
    c.on('data', (d) => { buf += d; const i = buf.indexOf('\n'); if (i >= 0) done(buf.slice(0, i)); });
    c.on('error', () => done(null));
    c.on('close', () => done(buf || null));
  });
}

function snapshot() {
  const j = herdrJson(['api', 'snapshot']);
  return j && j.result && j.result.snapshot;
}

async function main() {
  const stateDir = process.env.HERDR_PLUGIN_STATE_DIR || os.tmpdir();
  fs.mkdirSync(stateDir, { recursive: true });
  const stamp = path.join(stateDir, 'claude-worktrees.stamp');
  const lockDir = path.join(stateDir, 'claude-worktrees.lock');

  if (process.env.HERDR_PLUGIN_EVENT === 'pane.agent_status_changed') {
    try { if (Date.now() - fs.statSync(stamp).mtimeMs < 15000) return log('skip: ran <15s ago'); } catch (e) {}
  }
  if (!DRY) {
    try { fs.mkdirSync(lockDir); } catch (e) {
      try {
        if (Date.now() - fs.statSync(lockDir).mtimeMs > 120000) { fs.rmdirSync(lockDir); fs.mkdirSync(lockDir); } else return;
      } catch (e2) { return; }
    }
  }
  try {
    await sync();
    if (!DRY) fs.writeFileSync(stamp, String(Date.now()));
  } finally {
    if (!DRY) { try { fs.rmdirSync(lockDir); } catch (e) {} }
  }
}

async function sync() {
  let snap = snapshot();
  if (!snap) return log('no snapshot');
  const wss = snap.workspaces || [];
  const panes = snap.panes || [];
  const open = new Set(wss.filter((w) => w.worktree && w.worktree.checkout_path).map((w) => norm(w.worktree.checkout_path)));
  const reposOf = new Map(); // primary workspace_id -> Set of normalised repo roots

  // Step 1: open missing worktrees
  for (const ws of wss) {
    if (ws.worktree && ws.worktree.is_linked_worktree) continue;
    const repos = []; // {path, viaCwd}
    const lst = herdrJson(['worktree', 'list', '--workspace', ws.workspace_id]);
    const root = lst && lst.result && lst.result.source && lst.result.source.repo_root;
    if (root) repos.push({ path: root, viaCwd: false });
    else {
      const first = panes.find((p) => p.workspace_id === ws.workspace_id);
      const cwd = first && first.cwd;
      if (cwd) {
        try {
          for (const e of fs.readdirSync(cwd, { withFileTypes: true })) {
            if (e.isDirectory()) {
              const d = path.join(cwd, e.name);
              if (isGitTop(d)) repos.push({ path: d, viaCwd: true });
            }
          }
        } catch (e) {}
      }
    }
    const set = new Set();
    reposOf.set(ws.workspace_id, set);
    for (const repo of repos) {
      set.add(norm(repo.path));
      if (!fs.existsSync(path.join(repo.path, '.claude', 'worktrees'))) continue;
      for (const wt of repoWorktrees(repo.path)) {
        if (open.has(norm(wt))) continue;
        const args = ['worktree', 'open', repo.viaCwd ? '--cwd' : '--workspace', repo.viaCwd ? repo.path : ws.workspace_id, '--path', wt, '--no-focus'];
        const res = herdrDo(args);
        open.add(norm(wt));
        const label = path.basename(wt).replace(/^[^+]*\+/, '');
        if (DRY) { console.log('WOULD RUN: herdr workspace rename <newid> ' + label); continue; }
        const r = res && res.result;
        const newId = r && ((r.workspace && r.workspace.workspace_id) || r.workspace_id);
        if (newId) herdrJson(['workspace', 'rename', newId, label]);
      }
    }
  }

  if (!DRY) snap = snapshot() || snap;
  const wss2 = snap.workspaces || [];
  const panes2 = snap.panes || [];

  // Step 2: close dead worktree spaces
  const closed = new Set();
  for (const ws of wss2) {
    const cp = ws.worktree && ws.worktree.checkout_path;
    if (!cp || !norm(cp).includes('/.claude/worktrees/') || fs.existsSync(cp)) continue;
    const mine = panes2.filter((p) => p.workspace_id === ws.workspace_id);
    const allShells = mine.every((p) => {
      const r = herdrJson(['pane', 'process-info', '--pane', p.pane_id]);
      const procs = r && r.result && r.result.process_info && r.result.process_info.foreground_processes;
      if (!procs) return false;
      return procs.every((x) => SHELLS.has(String(x.name || '').toLowerCase().replace(/\.exe$/, '')));
    });
    if (allShells) { herdrDo(['workspace', 'close', ws.workspace_id]); closed.add(ws.workspace_id); }
  }

  // Step 3: ordering (primary space, then its repos' linked worktree spaces)
  const ordered = wss2.filter((w) => !closed.has(w.workspace_id)).slice().sort((a, b) => a.number - b.number);
  const isLinked = (w) => !!(w.worktree && w.worktree.is_linked_worktree);
  const repoOfLinked = (w) => {
    const wt = w.worktree || {};
    if (wt.repo_root) return norm(wt.repo_root);
    const n = norm(wt.checkout_path);
    const i = n.indexOf('/.claude/worktrees/');
    return i >= 0 ? n.slice(0, i) : '';
  };
  const kids = new Map();
  const placed = new Set();
  for (const w of ordered.filter(isLinked)) {
    const rr = repoOfLinked(w);
    const parent = ordered.find((p) => !isLinked(p) && ((reposOf.get(p.workspace_id) || new Set()).has(rr) || (p.worktree && norm(p.worktree.repo_root) === rr)));
    if (!parent) continue;
    if (!kids.has(parent.workspace_id)) kids.set(parent.workspace_id, []);
    kids.get(parent.workspace_id).push(w);
    placed.add(w.workspace_id);
  }
  const desired = [];
  for (const w of ordered) {
    if (placed.has(w.workspace_id)) continue;
    desired.push(w.workspace_id);
    for (const k of kids.get(w.workspace_id) || []) desired.push(k.workspace_id);
  }
  const current = ordered.map((w) => w.workspace_id);
  if (desired.join(',') === current.join(',')) return log('order unchanged: ' + current.join(','));
  if (DRY) return console.log('WOULD SEND workspace.move_block: ' + JSON.stringify(desired));
  await sendMove(desired);
}

main().catch(() => {}).then(() => process.exit(0));
