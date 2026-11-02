#!/usr/bin/env node
// Claude Code hook (SessionStart/SubagentStart/SubagentStop): shows running subagents in herdr's agents panel.
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const MAX = 15;
const san = (s) => String(s == null ? '' : s).replace(/[^A-Za-z0-9_-]/g, '_');
const sleep = (ms) => { try { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms); } catch (e) {} };
const herdrBin = () => process.env.HERDR_BIN_PATH || 'herdr';

function run(args) {
  return spawnSync(herdrBin(), args, { stdio: 'ignore', windowsHide: true });
}

function lock(dir) {
  const deadline = Date.now() + 5000;
  for (;;) {
    try { fs.mkdirSync(dir); return true; } catch (e) {
      try { if (Date.now() - fs.statSync(dir).mtimeMs > 30000) { fs.rmdirSync(dir); continue; } } catch (e2) {}
      if (Date.now() > deadline) return false;
      sleep(50);
    }
  }
}

function isParked(file) {
  try {
    const t = fs.readFileSync(file, 'utf8');
    const launched = new Set();
    for (const m of t.matchAll(/ID: (b[a-z0-9]{8})/g)) launched.add(m[1]);
    if (!launched.size) return false;
    const done = new Set();
    for (const m of t.matchAll(/<task-id>(b[a-z0-9]{8})<\/task-id>/g)) done.add(m[1]);
    for (const id of launched) if (!done.has(id)) return true;
  } catch (e) {}
  return false;
}

function findPaneByJob(jobDir) {
  try {
    const name = JSON.parse(fs.readFileSync(path.join(jobDir, 'state.json'), 'utf8')).name;
    if (!name) return null;
    const r = spawnSync(herdrBin(), ['agent', 'list'], { encoding: 'utf8', windowsHide: true, maxBuffer: 64 << 20 });
    const agents = JSON.parse(r.stdout).result.agents || [];
    const a = agents.find((x) => x.terminal_title_stripped === name);
    return a ? a.pane_id : null;
  } catch (e) { return null; }
}

function main() {
  if (process.env.HERDR_ENV !== '1' || !process.env.HERDR_PANE_ID) return;
  let input = {};
  try { input = JSON.parse(fs.readFileSync(0, 'utf8')); } catch (e) { return; }
  const ev = input.hook_event_name;
  const jobDir = process.env.CLAUDE_JOB_DIR;
  const key = san(jobDir ? 'job-' + path.basename(jobDir) : process.env.HERDR_PANE_ID);
  let pane = process.env.HERDR_PANE_ID;
  if (jobDir) { pane = findPaneByJob(jobDir); if (!pane) return; }

  const agentId = input.agent_id ? san(input.agent_id) : '';
  if (ev === 'SessionStart') { if (agentId) return; }
  else if (ev !== 'SubagentStart' && ev !== 'SubagentStop') return;
  else if (!agentId) return;

  const dir = path.join(os.tmpdir(), 'herdr-subagents', key);
  fs.mkdirSync(dir, { recursive: true });
  const lockDir = dir + '.lock';
  const locked = lock(lockDir);
  try {
    if (ev === 'SessionStart') {
      for (const f of fs.readdirSync(dir)) { try { fs.unlinkSync(path.join(dir, f)); } catch (e) {} }
    } else if (ev === 'SubagentStart') {
      fs.writeFileSync(path.join(dir, agentId), String(input.agent_type || 'subagent'));
    } else {
      let tp = input.agent_transcript_path;
      if (!tp && input.transcript_path) tp = input.transcript_path.replace(/\.jsonl$/, '') + '/subagents/agent-' + input.agent_id + '.jsonl';
      if (!(tp && isParked(tp))) { try { fs.unlinkSync(path.join(dir, agentId)); } catch (e) {} }
    }
    const names = fs.readdirSync(dir)
      .map((f) => { try { return { f, m: fs.statSync(path.join(dir, f)).mtimeMs, t: fs.readFileSync(path.join(dir, f), 'utf8').trim() || 'subagent' }; } catch (e) { return null; } })
      .filter(Boolean).sort((a, b) => a.m - b.m || (a.f < b.f ? -1 : 1))
      .map((x) => x.t.slice(0, 60));
    const n = names.length;
    const seq = String(Date.now() * 1000);
    const args = ['pane', 'report-metadata', pane, '--source', 'user:subagents', '--seq', seq, '--ttl-ms', '21600000'];
    for (let i = 1; i <= MAX; i++) {
      let label = null;
      if (n > MAX && i === MAX) label = '↳ +' + (n - (MAX - 1)) + ' more';
      else if (i <= n) label = '↳ ' + names[i - 1];
      if (label !== null) args.push('--token', 'sub' + i + '=' + label);
      else args.push('--clear-token', 'sub' + i);
    }
    run(args);
  } finally {
    if (locked) { try { fs.rmdirSync(lockDir); } catch (e) {} }
  }
}

try { main(); } catch (e) {}
process.exit(0);
