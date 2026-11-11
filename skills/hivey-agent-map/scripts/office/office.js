// hivey office: every live agent at a desk. Data: ./office.json (office.py, refreshed every 5 min by
// serve.py while this page is open). Three.js draws the office; the board and the list are plain HTML.
// Click an agent: open its terminal in hivey. Right-click (or ⓘ): details. Only working agents move.
import * as THREE from './three.module.min.js';
import { RoomEnvironment } from './RoomEnvironment.js';
import { OutlineEffect } from './OutlineEffect.js';
import { EffectComposer } from './postprocessing/EffectComposer.js';
import { RenderPass } from './postprocessing/RenderPass.js';
import { UnrealBloomPass } from './postprocessing/UnrealBloomPass.js';
import { OutputPass } from './postprocessing/OutputPass.js';
import { createShip } from './ship.js';

const $ = (s, el = document) => el.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const STATUS = {
  blocked: { label: 'Needs you', emoji: '✋', color: '#fb7185' },
  working: { label: 'Working', emoji: '⚡', color: '#38bdf8' },
  done: { label: 'Done', emoji: '✅', color: '#4ade80' },
  idle: { label: 'Idle', emoji: '💤', color: '#94a3b8' },
};
const st = s => STATUS[s] || { label: s || '?', emoji: '•', color: '#64748b' };
const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches;

let data = null, agents = [], selected = null;
const ui = JSON.parse(localStorage.getItem('hv-office-ship') || '{}');
ui.view ||= 'office';
ui.filter ||= null;
if (ui.toon === undefined) ui.toon = false;
const save = () => localStorage.setItem('hv-office-ship', JSON.stringify({ view: ui.view, filter: ui.filter, toon: ui.toon, music: ui.music }));

// ---------------------------------------------------------------- helpers
function hue(name) { let h = 0; for (const c of name || '') h = (h * 31 + c.charCodeAt(0)) >>> 0; return h % 360; }
function ago(iso) {
  if (!iso) return '';
  const s = Math.max(0, (Date.now() - Date.parse(iso)) / 1000);
  return s < 60 ? 'just now' : s < 3600 ? `${Math.round(s / 60)}m ago` : s < 86400 ? `${Math.round(s / 3600)}h ago` : `${Math.round(s / 86400)}d ago`;
}
function ktok(n) { return n >= 1e6 ? (n / 1e6).toFixed(2) + 'M' : n >= 1e3 ? Math.round(n / 1e3) + 'k' : String(n || 0); }
function ctxColor(p) { return p >= 80 ? '#fb7185' : p >= 60 ? '#f5b83d' : '#4ade80'; }
function model(m) {
  const k = /claude-(opus|sonnet|haiku)-(\d+)(?:-(\d{1,2}))?(?!\d)/.exec(m || '');
  return k ? `${k[1][0].toUpperCase() + k[1].slice(1)} ${k[2]}${k[3] ? '.' + k[3] : ''}` : (m || '').split('/').pop();
}
function ring(p, size = 18) {
  const r = size / 2 - 2, c = 2 * Math.PI * r, v = Math.min(100, p || 0);
  return `<svg width="${size}" height="${size}" viewBox="0 0 ${size} ${size}" style="flex:none"><circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="#273246" stroke-width="3"/>` +
    `<circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="${ctxColor(v)}" stroke-width="3" stroke-linecap="round" stroke-dasharray="${c * v / 100} ${c}" transform="rotate(-90 ${size / 2} ${size / 2})"/></svg>`;
}
function adviceBadge(a) {
  const s = a.summary || {};
  if (s.advice === 'compact') return `<span class="badge compact" title="${esc(s.advice_why)}">🗜 compact</span>`;
  if (s.advice === 'clear') return `<span class="badge clear" title="${esc(s.advice_why)}">🧹 clear</span>`;
  return '';
}
function toast(msg) { const t = $('#toast'); t.textContent = msg; t.classList.add('show'); clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('show'), 2600); }
function visible(a) {
  if (ui.filter) {
    if (ui.filter in STATUS && a.status !== ui.filter) return false;
    if ((ui.filter === 'compact' || ui.filter === 'clear') && a.summary?.advice !== ui.filter) return false;
  }
  const q = $('#q').value.trim().toLowerCase();
  if (!q) return true;
  const s = a.summary || {};
  return [a.name, a.space, a.cli, a.model, s.job, s.bubble, s.next, s.needs_you, ...(s.done || [])].join(' ').toLowerCase().includes(q);
}

// ---------------------------------------------------------------- actions
// click sound: a short sci-fi "warp ping" made with Web Audio (two rising tones and an echo), no file
let audioCtx = null;
function warpPing() {
  try {
    audioCtx ||= new (window.AudioContext || window.webkitAudioContext)();
    const ac = audioCtx, t = ac.currentTime, out = ac.createGain();
    out.gain.value = 0.5;
    const echo = ac.createDelay(), fb = ac.createGain();
    echo.delayTime.value = 0.13; fb.gain.value = 0.32;
    out.connect(ac.destination); out.connect(echo); echo.connect(fb); fb.connect(echo); echo.connect(ac.destination);
    [['sine', 880, 1760, 0, 0.26, 0.32], ['triangle', 1320, 2640, 0.07, 0.12, 0.4], ['sine', 440, 660, 0, 0.1, 0.5]].forEach(([type, f0, f1, at, vol, len]) => {
      const o = ac.createOscillator(), g = ac.createGain();
      o.type = type; o.frequency.setValueAtTime(f0, t + at); o.frequency.exponentialRampToValueAtTime(f1, t + at + 0.16);
      g.gain.setValueAtTime(0.0001, t + at); g.gain.exponentialRampToValueAtTime(vol, t + at + 0.02); g.gain.exponentialRampToValueAtTime(0.0001, t + at + len);
      o.connect(g); g.connect(out); o.start(t + at); o.stop(t + at + len + 0.05);
    });
    setTimeout(() => { fb.gain.value = 0; }, 900);
  } catch (e) { /* no audio: the flash is enough */ }
}
async function act(a, action) {
  if (action === 'focus') {
    warpPing();
    toast(`Opening ${a.name} in hivey…`);
    office.flash(a.id);
    document.querySelectorAll(`[data-id="${CSS.escape(a.id)}"]`).forEach(el => { el.classList.remove('flash'); void el.offsetWidth; el.classList.add('flash'); });
  }
  try {
    const r = await fetch('/hv-office-action', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ pane: a.pane, action }) });
    const j = await r.json();
    toast(j.ok ? (action === 'focus' ? `${a.name} is open in hivey ↗` : `${a.name}: ${j.message}`) : `${a.name}: ${j.message}`);
  } catch (e) { toast('The office server is not answering: run `hivey agent-map office` again'); }
}
// compact / clear need a second click within 3 s (no browser dialogs)
function armed(btn, a, action) {
  if (btn.classList.contains('arm')) { btn.classList.remove('arm'); btn.textContent = btn.dataset.label; act(a, action); return; }
  btn.dataset.label = btn.textContent;
  btn.classList.add('arm');
  btn.textContent = `Click again to ${action}`;
  setTimeout(() => { if (btn.classList.contains('arm')) { btn.classList.remove('arm'); btn.textContent = btn.dataset.label; } }, 3000);
}
function actionButtons(a) {
  const busy = a.status === 'working' || a.status === 'blocked';
  return `<div class="actions">
    <button class="primary" data-act="focus" title="Open its terminal in hivey">↗ Open</button>
    ${a.cli_id === 'claude' ? `<button data-act="compact" ${busy ? 'disabled title="Only when it is not working"' : 'title="Type /compact into it"'}>🗜 Compact</button>
    <button data-act="clear" ${busy ? 'disabled title="Only when it is not working"' : 'title="Type /clear into it"'}>🧹 Clear</button>` : ''}
    <button data-act="info" title="Everything about it">ⓘ Details</button></div>`;
}
function wireActions(root) {
  root.querySelectorAll('[data-id]').forEach(el => {
    const a = agents.find(x => x.id === el.dataset.id);
    if (!a) return;
    el.addEventListener('click', e => {
      const b = e.target.closest('button[data-act]');
      if (!b) return act(a, 'focus');
      e.stopPropagation();
      if (b.disabled) return;
      const k = b.dataset.act;
      if (k === 'info') return openDrawer(a);
      if (k === 'focus') return act(a, 'focus');
      armed(b, a, k);
    });
    el.addEventListener('contextmenu', e => { e.preventDefault(); openDrawer(a); });
  });
}

// ---------------------------------------------------------------- header, inbox
function renderHeader() {
  const c = data.counts || {}, adv = data.advice || {};
  const pills = [
    ['blocked', '✋', 'needs you', c.blocked], ['working', '⚡', 'working', c.working], ['done', '✅', 'done', c.done],
    ['idle', '💤', 'idle', c.idle], ['compact', '🗜', 'to compact', adv.compact], ['clear', '🧹', 'to clear', adv.clear],
  ];
  $('#pills').innerHTML = pills.map(([k, e, l, n]) =>
    `<button class="pill ${k} ${ui.filter === k ? 'on' : ''}" data-f="${k}" title="Show only these">${e} <b>${n || 0}</b> ${l}</button>`).join('');
  $('#pills').querySelectorAll('.pill').forEach(p => p.onclick = () => { ui.filter = ui.filter === p.dataset.f ? null : p.dataset.f; save(); renderAll(); });
  tickSub();
}
function tickSub() {
  if (!data) return;
  const next = Date.parse(data.generated) / 1000 + (data.refresh_every || 300) - Date.now() / 1000;
  const n = agents.length;
  const mm = Math.max(0, Math.floor(next / 60)), ss = Math.max(0, Math.floor(next % 60));
  $('#sub').innerHTML = `${n} agent${n === 1 ? '' : 's'} · updated ${ago(data.generated)} · ` +
    (refreshing ? '<span class="spin">⟳</span> reading terminals…' : next > 0 ? `next in ${mm}:${String(ss).padStart(2, '0')}` : 'refreshing soon') +
    (data.error ? ` · <span class="err" title="${esc(data.error)}">summaries from the recap (model call failed)</span>` : '');
}
function renderInbox() {
  const blocked = agents.filter(a => a.status === 'blocked');
  const asked = agents.filter(a => a.status !== 'blocked' && a.status !== 'working' && a.summary?.needs_you);
  const need = [...blocked, ...asked];
  const lead = [blocked.length ? `✋ ${blocked.length} blocked` : '', asked.length ? `<span style="color:var(--honey)">💬 ${asked.length} asked you</span>` : ''].filter(Boolean).join('<br>');
  $('#inbox').innerHTML = need.length ? `<span class="lead">${lead}</span>` + need.map(a =>
    `<div class="ask ${a.status === 'blocked' ? '' : 'soft'}" data-id="${esc(a.id)}" title="Click: open in hivey · right-click: details"><span class="em">${esc(a.emoji)}</span>
     <div class="t"><b>${esc(a.name)}</b>${esc(a.summary?.needs_you || a.summary?.bubble || 'waiting in its terminal')}</div><span class="go">open ↗</span></div>`).join('') : '';
  wireActions($('#inbox'));
}

// ---------------------------------------------------------------- board & list
function card(a) {
  const s = a.summary || {}, h = hue(a.space), S = st(a.status);
  return `<div class="card ${esc(a.status)}" data-id="${esc(a.id)}" title="Click: open in hivey · right-click: details">
    <div class="top"><div class="avatar" style="--hue1:hsl(${h} 70% 62%);--hue2:hsl(${h} 55% 32%)">${esc(a.emoji)}<span class="st" style="background:${S.color}"></span></div>
      <div style="min-width:0"><div class="name">${esc(a.name)}</div>
      <div class="meta">${S.emoji} ${S.label} · ${esc(a.cli)}${a.model ? ' · ' + esc(model(a.model)) : ''}${a.team?.length ? ` · 👥 ${a.team.length}` : ''} · ${ago(a.last_activity)}</div></div></div>
    <div class="say">${esc(s.bubble || '…')}</div>
    <div class="job">📌 ${esc(s.job || '')}</div>
    ${s.needs_you ? `<div class="need">✋ ${esc(s.needs_you)}</div>` : ''}
    ${(s.done || []).length ? `<ul>${s.done.map(d => `<li>${esc(d)}</li>`).join('')}</ul>` : ''}
    ${s.next ? `<div class="next">→ ${esc(s.next)}</div>` : ''}
    <div class="ctx">🧠 <span class="bar"><i style="width:${Math.min(100, a.context_pct)}%;background:${ctxColor(a.context_pct)}"></i></span>
      ${a.context ? `${a.context_pct}% of ${ktok(a.window)}` : 'no session log'} ${adviceBadge(a)}</div>
    ${actionButtons(a)}</div>`;
}
function renderBoard() {
  const list = agents.filter(visible);
  const groups = [['blocked', '✋ Needs you'], ['working', '⚡ Working now'], ['done', '✅ Finished: review'], ['idle', '💤 Idle']];
  const other = list.filter(a => !STATUS[a.status]);
  $('#board').innerHTML = list.length ? groups.map(([k, t]) => {
    const g = list.filter(a => a.status === k);
    return g.length ? `<div class="section"><h2>${t} <span>· ${g.length}</span></h2><div class="grid">${g.map(card).join('')}</div></div>` : '';
  }).join('') + (other.length ? `<div class="section"><h2>Other · ${other.length}</h2><div class="grid">${other.map(card).join('')}</div></div>` : '')
    : '<div class="empty">No agent matches.</div>';
  wireActions($('#board'));
}
let sortKey = 'status', sortDir = 1;
function renderList() {
  const order = { blocked: 0, working: 1, done: 2, idle: 3 };
  const key = {
    status: a => order[a.status] ?? 9, name: a => a.name, space: a => a.space, job: a => a.summary?.job || '',
    context: a => -a.context_pct, advice: a => ({ compact: 0, clear: 1 }[a.summary?.advice] ?? 2), active: a => -(Date.parse(a.last_activity) || 0),
  }[sortKey];
  const list = agents.filter(visible).sort((a, b) => { const x = key(a), y = key(b); return (x < y ? -1 : x > y ? 1 : 0) * sortDir; });
  const th = (k, l) => `<th data-k="${k}">${l}${sortKey === k ? (sortDir > 0 ? ' ▲' : ' ▼') : ''}</th>`;
  $('#list').innerHTML = `<table><thead><tr>${th('status', '')}${th('name', 'Agent')}${th('space', 'Space')}${th('job', 'Current job · says')}${th('context', 'Context')}${th('advice', 'Advice')}${th('active', 'Active')}<th></th></tr></thead><tbody>` +
    list.map(a => { const s = a.summary || {}; return `<tr data-id="${esc(a.id)}" title="Click: open in hivey · right-click: details">
      <td>${st(a.status).emoji}</td><td><b>${esc(a.name)}</b><div class="meta">${esc(a.cli)}${a.model ? ' · ' + esc(model(a.model)) : ''}${a.team?.length ? ' · 👥 ' + a.team.length : ''}</div></td>
      <td>${esc(a.emoji)} ${esc(a.space)}</td><td class="j"><b>${esc(s.job)}</b><div class="meta">${esc(s.needs_you ? '✋ ' + s.needs_you : s.bubble)}</div></td>
      <td><span style="display:flex;gap:6px;align-items:center">${ring(a.context_pct)} ${a.context ? a.context_pct + '%' : '–'}</span></td>
      <td>${adviceBadge(a)}</td><td class="meta">${ago(a.last_activity)}</td><td><button class="iconbtn" data-act="info" style="padding:3px 8px">ⓘ</button></td></tr>`; }).join('') +
    '</tbody></table>' + (list.length ? '' : '<div class="empty">No agent matches.</div>');
  $('#list').querySelectorAll('th[data-k]').forEach(h => h.onclick = () => { sortDir = sortKey === h.dataset.k ? -sortDir : 1; sortKey = h.dataset.k; renderList(); });
  wireActions($('#list'));
}

// ---------------------------------------------------------------- drawer
function openDrawer(a) {
  selected = a.id;
  const s = a.summary || {}, S = st(a.status);
  $('#drawer').innerHTML = `<button class="x" title="Close (Esc)">✕</button>
    <div style="display:flex;gap:12px;align-items:center"><div class="avatar" style="--hue1:hsl(${hue(a.space)} 70% 62%);--hue2:hsl(${hue(a.space)} 55% 32%)">${esc(a.emoji)}</div>
    <div><h3>${esc(a.name)}</h3><div class="meta">${S.emoji} ${S.label} · ${esc(a.role)} · ${esc(a.cli)}${a.model ? ' · ' + esc(model(a.model)) : ''}</div></div></div>
    <div class="say" style="margin:14px 0;padding:10px 12px;border-radius:12px;background:var(--panel2)">${esc(s.bubble)}</div>
    ${s.needs_you ? `<div class="need" style="padding:8px 10px;border-radius:10px;background:rgba(251,113,133,.12);border:1px solid rgba(251,113,133,.35)">✋ ${esc(s.needs_you)}</div>` : ''}
    <div class="gauge">${ring(a.context_pct, 64)}<div><b>${a.context ? a.context_pct + '% context' : 'no session log'}</b>
      <div class="meta">${a.context ? `${ktok(a.context)} of ${ktok(a.window)} tokens` : 'context is read from Claude Code session logs'}</div>
      <div style="margin-top:4px">${adviceBadge(a) || '<span class="badge">👍 keep</span>'} <span class="meta">${esc(s.advice_why)}</span></div></div></div>
    ${actionButtons(a)}
    <dl>
      <dt>Current job</dt><dd>📌 ${esc(s.job)}</dd>
      ${(s.done || []).length ? `<dt>Achieved</dt><dd>${s.done.map(d => '✓ ' + esc(d)).join('<br>')}</dd>` : ''}
      ${s.next ? `<dt>Next</dt><dd>→ ${esc(s.next)}</dd>` : ''}
      ${a.about ? `<dt>About</dt><dd>${esc(a.about)}</dd>` : ''}
      ${a.team?.length ? `<dt>Swarm team</dt><dd>${a.team.map(m => `${st(m.status).emoji} ${esc(m.name)} <span class="meta">${esc(m.role)}</span>`).join('<br>')}</dd>` : ''}
      ${a.last_prompt ? `<dt>Your last prompt</dt><dd>${esc(a.last_prompt)}</dd>` : ''}
      ${a.recap ? `<dt>Session recap</dt><dd>${esc(a.recap)}</dd>` : ''}
      ${a.tools?.length ? `<dt>Recent tools</dt><dd class="meta">${a.tools.map(esc).join('<br>')}</dd>` : ''}
      <dt>Last active</dt><dd>${ago(a.last_activity) || '–'}${a.session_started ? ` · session since ${new Date(a.session_started).toLocaleString()}` : ''}</dd>
      <dt>Where</dt><dd>${esc(a.space_label)} · pane ${esc(a.pane)} · ${esc(a.cwd)}</dd>
      ${a.log ? `<dt>Session log</dt><dd class="meta">${esc(a.log)}</dd>` : ''}
      <dt>Summary</dt><dd class="meta">by ${esc(s.by || '?')} · ${ago(a.summarized_at)}</dd>
    </dl>
    <div class="meta" style="margin-bottom:6px">Terminal (bottom of the screen at the last refresh)</div>
    <pre>${esc(a.screen)}</pre>`;
  $('#drawer').classList.add('open');
  $('#drawer .x').onclick = closeDrawer;
  const el = $('#drawer');
  el.dataset.id = a.id;
  wireDrawer(a);
  office.markSelected();
}
function wireDrawer(a) {
  $('#drawer').querySelectorAll('button[data-act]').forEach(b => b.onclick = e => {
    e.stopPropagation();
    if (b.disabled) return;
    const k = b.dataset.act;
    if (k === 'focus') return act(a, 'focus');
    if (k === 'info') return;
    armed(b, a, k);
  });
}
function closeDrawer() { $('#drawer').classList.remove('open'); selected = null; office.markSelected(); }

// ---------------------------------------------------------------- the 3D office
const office = (() => {
  const host = $('#office'), labels = $('#labels');
  let renderer;
  try { renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true }); } catch (e) { return { ok: false, build() {}, markSelected() {}, resize() {}, fit() {}, flash() {} }; }
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.05;
  host.prepend(renderer.domElement);
  const scene = new THREE.Scene();
  const envMap = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(renderer), 0.04).texture;
  // comic mode: three-tone cel shading + ink outlines (OutlineEffect = inverted hulls in the shader)
  const outline = new OutlineEffect(renderer, { defaultThickness: 0.0045, defaultColor: [0.09, 0.07, 0.08], defaultAlpha: 1 });
  const grad = new THREE.DataTexture(new Uint8Array([90, 90, 90, 255, 175, 175, 175, 255, 255, 255, 255, 255]), 3, 1, THREE.RGBAFormat);
  grad.minFilter = grad.magFilter = THREE.NearestFilter; grad.needsUpdate = true;
  const toonCache = new Map();
  const NO_LINE = { visible: false };
  function toonOf(m) {
    if (!m.isMeshStandardMaterial) return m;
    let t = toonCache.get(m);
    if (!t) {
      t = new THREE.MeshToonMaterial({ color: m.color, map: m.map, emissive: m.emissive, emissiveIntensity: m.emissiveIntensity, gradientMap: grad,
        transparent: m.transparent, opacity: m.opacity, side: m.side, depthWrite: m.depthWrite });
      t.userData.outlineParameters = m.userData.outlineParameters || (m.transparent ? NO_LINE : undefined);
      toonCache.set(m, t);
    }
    return t;
  }
  function applyStyle(root) {
    scene.environment = ui.toon ? null : envMap;
    hemi.intensity = ui.toon ? 1.3 : 0.35;
    sun.intensity = ui.toon ? 1.8 : 2.0;
    renderer.toneMapping = ui.toon ? THREE.NoToneMapping : THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 0.82;
    if (!ui.toon) return;
    root.traverse(o => { if (o.isMesh && o.material) o.material = toonOf(o.material); });
  }
  const cam = new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 500);
  const target = new THREE.Vector3();
  const camDir = new THREE.Vector3(0.36, 0.82, 1.3).normalize();
  let zoom = 1, bounds = { w: 20, d: 20, cx: 0, cz: 0 };
  const hemi = new THREE.HemisphereLight(0xdfe8ff, 0x3a2f28, 0.45); scene.add(hemi);
  const sun = new THREE.DirectionalLight(0xdfe9ff, 2.0);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  sun.shadow.bias = -0.0004;
  sun.shadow.normalBias = 0.02;
  sun.shadow.radius = 4;
  scene.add(sun, sun.target);

  // shared geometries and materials
  const G = {
    box: new THREE.BoxGeometry(1, 1, 1), body: new THREE.CapsuleGeometry(0.34, 0.42, 6, 14), head: new THREE.SphereGeometry(0.36, 22, 16),
    visor: new THREE.SphereGeometry(0.37, 22, 12, -Math.PI * 0.42, Math.PI * 0.84, Math.PI * 0.3, Math.PI * 0.32),
    eye: new THREE.SphereGeometry(0.055, 10, 8), arm: new THREE.CapsuleGeometry(0.07, 0.34, 4, 8), ant: new THREE.CylinderGeometry(0.018, 0.018, 0.3, 6),
    tip: new THREE.SphereGeometry(0.06, 10, 8), wing: new THREE.CircleGeometry(0.32, 20), stripe: new THREE.TorusGeometry(0.345, 0.045, 8, 26),
    plane: new THREE.PlaneGeometry(1, 1), cyl: new THREE.CylinderGeometry(0.05, 0.05, 1, 8), seat: new THREE.CylinderGeometry(0.34, 0.34, 0.08, 18),
    hit: new THREE.BoxGeometry(2.4, 2.4, 2.4), ring: new THREE.RingGeometry(0.5, 0.62, 30),
    mug: new THREE.CylinderGeometry(0.08, 0.07, 0.17, 12), pot: new THREE.CylinderGeometry(0.11, 0.09, 0.18, 10), leaf: new THREE.IcosahedronGeometry(0.2, 0),
  };
  const M = {
    floor: new THREE.MeshStandardMaterial({ color: 0x0b0e15, roughness: 1 }),
    wood: new THREE.MeshStandardMaterial({ color: 0xa47a52, roughness: 0.65 }),
    dark: new THREE.MeshStandardMaterial({ color: 0x1d2433, roughness: 0.5, metalness: 0.3 }),
    metal: new THREE.MeshStandardMaterial({ color: 0x8a94a6, roughness: 0.35, metalness: 0.7 }),
    visor: new THREE.MeshStandardMaterial({ color: 0x0b1020, roughness: 0.15, metalness: 0.6 }),
    stripe: new THREE.MeshStandardMaterial({ color: 0x151515, roughness: 0.6 }),
    wing: new THREE.MeshStandardMaterial({ color: 0xdff3ff, transparent: true, opacity: 0.45, side: THREE.DoubleSide, roughness: 0.1 }),
    hit: new THREE.MeshBasicMaterial({ visible: false }),
    wall: new THREE.MeshStandardMaterial({ color: 0x6f8fb0, roughness: 0.1, metalness: 0.2, transparent: true, opacity: 0.12, depthWrite: false }),
    leaf: new THREE.MeshStandardMaterial({ color: 0x3f9d5a, flatShading: true }),
  };
  const statusMat = {}, bodyMat = {}, rugMat = {};
  const matFor = (cache, key, make) => cache[key] || (cache[key] = make());
  const eyeMat = s => matFor(statusMat, s, () => new THREE.MeshStandardMaterial({ color: st(s).color, emissive: st(s).color, emissiveIntensity: s === 'idle' ? 0.25 : 1.4 }));
  const bodyMatFor = h => matFor(bodyMat, h, () => new THREE.MeshStandardMaterial({ color: new THREE.Color(`hsl(${h}, 72%, 58%)`), roughness: 0.4, metalness: 0.1 }));
  const rugMatFor = h => matFor(rugMat, h, () => { const m = new THREE.MeshStandardMaterial({ color: new THREE.Color(`hsl(${h}, 45%, 16%)`), roughness: 0.35, metalness: 0.5, transparent: true, opacity: 0.85 }); m.userData.outlineParameters = NO_LINE; return m; });
  const accentMat = {};
  const accentFor = h => matFor(accentMat, h, () => new THREE.MeshStandardMaterial({ color: `hsl(${h},70%,55%)`, emissive: `hsl(${h},85%,55%)`, emissiveIntensity: 1.25, toneMapped: false }));

  // one animated "code" texture shared by every working agent's hologram screen
  const codeCanvas = document.createElement('canvas');
  codeCanvas.width = 256; codeCanvas.height = 160;
  const cg = codeCanvas.getContext('2d');
  const codeTex = new THREE.CanvasTexture(codeCanvas);
  codeTex.colorSpace = THREE.SRGBColorSpace;
  const codeLines = Array.from({ length: 14 }, () => [Math.random() * 30, 40 + Math.random() * 160, Math.random()]);
  let codeScroll = 0;
  function drawCode() {
    cg.fillStyle = 'rgba(6,18,32,0.92)'; cg.fillRect(0, 0, 256, 160);
    cg.strokeStyle = 'rgba(56,189,248,.8)'; cg.lineWidth = 3; cg.strokeRect(1.5, 1.5, 253, 157);
    codeScroll = (codeScroll + 1) % 11;
    codeLines.forEach(([x, w, c], i) => {
      const y = 12 + i * 11 - codeScroll;
      if (y < 6 || y > 152) return;
      cg.fillStyle = c < 0.33 ? '#38bdf8' : c < 0.66 ? '#a5f3fc' : '#f5b83d';
      cg.fillRect(10 + x, y, w, 5);
    });
    codeTex.needsUpdate = true;
  }
  drawCode();
  const holoMat = new THREE.MeshBasicMaterial({ map: codeTex, transparent: true, opacity: 0.6, side: THREE.DoubleSide, toneMapped: false, depthWrite: false });
  const lidMats = {};
  const lidMat = s => matFor(lidMats, s, () => new THREE.MeshStandardMaterial({ color: 0x2a3344, emissive: st(s).color, emissiveIntensity: s === 'idle' ? 0.05 : 0.55, roughness: 0.4 }));
  function signTexture(text, color) {
    const c = document.createElement('canvas'); c.width = c.height = 128;
    const g = c.getContext('2d');
    g.fillStyle = color; g.beginPath(); g.arc(64, 64, 56, 0, Math.PI * 2); g.fill();
    g.fillStyle = '#fff'; g.font = 'bold 76px system-ui'; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText(text, 64, 70);
    const t = new THREE.CanvasTexture(c); t.colorSpace = THREE.SRGBColorSpace; return t;
  }
  const signs = {
    blocked: new THREE.SpriteMaterial({ map: signTexture('!', '#f43f5e'), depthTest: false }),
    done: new THREE.SpriteMaterial({ map: signTexture('✓', '#22c55e'), depthTest: false }),
    idle: new THREE.SpriteMaterial({ map: signTexture('z', '#475569'), depthTest: false, opacity: 0.8, transparent: true }),
  };

  function mesh(geo, mat, x = 0, y = 0, z = 0, sx = 1, sy = 1, sz = 1, shadow = true) {
    const m = new THREE.Mesh(geo, mat); m.position.set(x, y, z); m.scale.set(sx, sy, sz);
    m.castShadow = shadow; m.receiveShadow = true; return m;
  }
  const ship = createShip({ mesh, hue, st });
  scene.background = ship.background; // the ship floats in deep space
  [M.wall, M.floor, M.hit, holoMat].forEach(m => m.userData.outlineParameters = NO_LINE);
  Object.values(signs).forEach(m => m.userData.outlineParameters = NO_LINE);
  // HD: glow on LEDs, screens and stars (bloom), then tone mapping
  const composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(scene, cam));
  const bloom = new UnrealBloomPass(new THREE.Vector2(256, 256), 0.42, 0.4, 0.96);
  composer.addPass(bloom);
  composer.addPass(new OutputPass());


  function makeDesk(a, x, z) {
    const h = hue(a.space), g = new THREE.Group();
    g.position.set(x, 0, z);
    const station = ship.makeStation(g, a, h, holoMat);
    const b = ship.makeRobot(h, a.status, false, a.name);
    b.bot.position.set(0, b.hover ? 0.2 : 0.02, -0.9);
    if (b.glb) b.bot.position.set(0, b.seated ? 0.36 : 0, b.seated ? -1.05 : -0.85);
    if (b.hover) station.chair.visible = false;
    g.add(b.bot);
    // coordinator: its swarm team stands beside the desk
    const team = [];
    (a.team || []).slice(0, 6).forEach((m, i) => {
      const t = ship.makeRobot(h, m.status, true, a.name + '/' + m.name);
      t.bot.position.set(1.35 + (i % 2) * 0.45, 0, -0.75 + Math.floor(i / 2) * 0.5);
      t.bot.rotation.y = -0.5; t.bot.position.y += t.hover ? 0.25 : 0.05;
      g.add(t.bot); team.push({ ...t, status: m.status, ph: Math.random() * 6 });
    });
    // status things: hologram for working, a sign above everyone else
    let holo = null, sign = null;
    if (a.status === 'working') {
      holo = station.screen;
    } else if (signs[a.status]) {
      sign = new THREE.Sprite(signs[a.status]); sign.position.set(0.85, 2.05, -0.9); sign.scale.setScalar(a.status === 'blocked' ? 0.55 : 0.4);
      sign.renderOrder = 5; g.add(sign);
    }
    const ring = mesh(G.ring, new THREE.MeshBasicMaterial({ color: 0xf5b83d, transparent: true, opacity: 0, side: THREE.DoubleSide }), 0, 0.02, -0.85, 1.4, 1.4, 1.4, false);
    ring.rotation.x = -Math.PI / 2; g.add(ring);
    const hit = mesh(G.hit, M.hit, 0, 1.1, -0.4, 1, 1, 1, false); hit.userData.id = a.id; g.add(hit);
    if (a.status === 'idle') { b.head.rotation.x = 0.08; b.arms.forEach(p => { p.rotation.x = -0.3; }); } // powered down: head bowed
    if (a.status === 'done') { b.arms.forEach((p, i) => { p.rotation.x = -1.35; p.rotation.z = (i ? -1 : 1) * 0.9; }); } // arms folded
    if (a.status === 'blocked' && b.arms[1]) { b.arms[1].rotation.x = -2.9; b.arms[1].rotation.z = 0.3; }
    return { g, bot: b, holo, sign, team, ring, hit, a, ph: Math.random() * 6, anchor: new THREE.Vector3(x, 2.5, z - 0.9) };
  }



  let world = new THREE.Group(), desks = [], rooms = [], tags = new Map(), roomTags = [], ordered = [];
  scene.add(world);
  const floor = mesh(new THREE.PlaneGeometry(1, 1), M.floor, 0, -0.01, 0, 1, 1, 1, false);
  floor.rotation.x = -Math.PI / 2; scene.add(floor);
  const grid = new THREE.GridHelper(1, 1, 0x1f2838, 0x1a2230); scene.add(grid);

  function addTag(a, d) {
    const el = document.createElement('div');
    el.className = `tag ${a.status} ${a.status === 'blocked' || a.status === 'working' ? 'hot' : ''}`;
    const s = a.summary || {};
    el.innerHTML = `<div class="bubble">${esc(s.needs_you && a.status === 'blocked' ? '✋ ' + s.needs_you : s.bubble || s.job || '…')}</div>
      <div class="plate"><span class="dot" style="background:${st(a.status).color}"></span><b>${esc(a.name)}</b>${a.team?.length ? ` 👥${a.team.length}` : ''} ${a.context ? ring(a.context_pct, 14) + a.context_pct + '%' : ''}</div>`;
    labels.appendChild(el);
    tags.set(a.id, { el, d, prio: ({ blocked: 0, working: 1, done: 2, idle: 3 }[a.status] ?? 4) - (a.summary?.needs_you ? 0.5 : 0) });
  }
  function build(list) {
    scene.remove(world); world = new THREE.Group(); scene.add(world);
    desks = []; labels.innerHTML = ''; tags = new Map(); roomTags = [];
    // the CEO's space gets the captain's bridge; everyone else gets team rooms
    const isCeo = a => /^ceo$/i.test(a.space || '');
    const ceo = list.filter(isCeo);
    list = list.filter(a => !isCeo(a));
    // rooms = spaces, most urgent first; desks in rows of 3 inside each room
    const order = { blocked: 0, working: 1, done: 2, idle: 3 };
    const bySpace = new Map();
    list.forEach(a => { if (!bySpace.has(a.workspace)) bySpace.set(a.workspace, []); bySpace.get(a.workspace).push(a); });
    const spaces = [...bySpace.values()].sort((x, y) => Math.min(...x.map(a => order[a.status] ?? 9)) - Math.min(...y.map(a => order[a.status] ?? 9)) || y.length - x.length);
    const DX = 4.2, DZ = 4.4, PAD = 1.2, GAP = 1.6;
    const sizes = spaces.map(s => { const cols = Math.min(3, s.length), rows = Math.ceil(s.length / 3), wide = s.some(a => a.team?.length);
      return { s, cols, rows, w: cols * DX + PAD * 2 + (wide ? 1.2 : 0), d: rows * DZ + PAD * 2 }; });
    const area = sizes.reduce((t, r) => t + (r.w + GAP) * (r.d + GAP), 0);
    const maxW = Math.max(Math.sqrt(area) * 1.5, ...sizes.map(r => r.w));
    let x = 0, z = 0, rowD = 0, W = 0;
    sizes.forEach(r => {
      if (x > 0 && x + r.w > maxW) { x = 0; z += rowD + GAP; rowD = 0; }
      r.x = x; r.z = z; x += r.w + GAP; rowD = Math.max(rowD, r.d); W = Math.max(W, x - GAP);
    });
    const D = z + rowD;
    const ox = -W / 2, oz = -D / 2;
    sizes.forEach(r => {
      const h = hue(r.s[0].space), rx = ox + r.x, rz = oz + r.z;
      const rug = mesh(G.plane, rugMatFor(h), rx + r.w / 2, 0.006, rz + r.d / 2, r.w - 0.3, r.d - 0.3, 1, false);
      rug.rotation.x = -Math.PI / 2; world.add(rug);
      world.add(mesh(G.box, M.wall, rx + r.w / 2, 0.5, rz + 0.06, r.w, 1.0, 0.06, false), mesh(G.box, M.wall, rx + 0.06, 0.5, rz + r.d / 2, 0.06, 1.0, r.d, false));
      ship.pod(world, rx + r.w - 0.6, rz + 0.6);
      world.add(mesh(G.box, accentFor(h), rx + r.w / 2, 1.02, rz + 0.06, r.w, 0.05, 0.1, false), mesh(G.box, accentFor(h), rx + 0.06, 1.02, rz + r.d / 2, 0.1, 0.05, r.d, false));
      r.s.sort((a, b) => (b.team?.length || 0) - (a.team?.length || 0) || (order[a.status] ?? 9) - (order[b.status] ?? 9));
      r.s.forEach((a, i) => {
        const d = makeDesk(a, rx + PAD + DX / 2 + (i % 3) * DX, rz + PAD + 1.6 + Math.floor(i / 3) * DZ);
        world.add(d.g); desks.push(d); addTag(a, d);
      });
      const rl = document.createElement('div');
      rl.className = 'room-label'; rl.textContent = `${r.s[0].emoji} ${r.s[0].space}`;
      labels.appendChild(rl); roomTags.push({ el: rl, p: new THREE.Vector3(rx + 0.3, 0.9, rz + r.d + 0.2) });
    });
    const e = ship.decorate(world, Math.max(W, 8), Math.max(D, 8));
    const br = ship.bridge(world, e);
    ceo.sort((a, b) => (a.role === 'session') - (b.role === 'session') || (order[a.status] ?? 9) - (order[b.status] ?? 9));
    ceo.slice(0, br.slots.length).forEach((a, i) => {
      const slot = br.slots[i], d = makeDesk(a, slot.x, slot.z);
      if (slot.captain) { d.g.scale.setScalar(1.18); d.anchor.y *= 1.18; }
      world.add(d.g); desks.push(d); addTag(a, d);
    });
    const bl = document.createElement('div');
    bl.className = 'room-label'; bl.textContent = `${ceo[0]?.emoji || '👔'} ceo · captain's bridge`;
    labels.appendChild(bl); roomTags.push({ el: bl, p: br.label });
    e.maxX = Math.max(e.maxX, br.bounds.maxX);
    // measure each label once (bubble and plate), for the overlap pass in placeLabels
    tags.forEach(t => {
      const b = t.el.querySelector('.bubble'), pl = t.el.querySelector('.plate');
      t.el.style.display = 'flex';
      t.bw = b.offsetWidth; t.bh = b.offsetHeight + 4; t.pw = pl.offsetWidth; t.ph = pl.offsetHeight;
    });
    ordered = [...tags.values()].sort((x, y) => x.prio - y.prio);
    bounds = { w: e.maxX - e.minX, d: e.maxZ - e.minZ, cx: (e.minX + e.maxX) / 2, cz: (e.minZ + e.maxZ) / 2 };
    const big = Math.max(bounds.w, bounds.d) * 3 + 40;
    floor.visible = false;
    grid.visible = false;
    const span = Math.max(bounds.w, bounds.d) * 0.62;
    sun.position.set(bounds.cx + span * 0.55, span * 1.1 + 10, bounds.cz + span * 0.75); sun.target.position.set(bounds.cx, 0, bounds.cz);
    Object.assign(sun.shadow.camera, { left: -span, right: span, top: span, bottom: -span, near: 1, far: span * 4 + 40 });
    sun.shadow.camera.updateProjectionMatrix();
    applyStyle(world);
    markSelected();
    if (!build.done) { fit(); build.done = true; }
    dirty = true;
  }

  function resize() {
    const w = host.clientWidth, h = host.clientHeight;
    renderer.setSize(w, h);
    composer.setPixelRatio(renderer.getPixelRatio()); composer.setSize(w, h);
    const aspect = w / Math.max(1, h);
    const half = 10 / zoom;
    cam.left = -half * aspect; cam.right = half * aspect; cam.top = half; cam.bottom = -half;
    cam.updateProjectionMatrix();
    dirty = true;
  }
  function placeCam() {
    cam.position.copy(target).addScaledVector(camDir, 120);
    cam.lookAt(target);
    dirty = true;
  }
  // fit = project the office's corners into the camera's view and frame them with a margin
  function fit() {
    target.set(bounds.cx, 0, bounds.cz);
    placeCam(); cam.updateMatrixWorld();
    const inv = cam.matrixWorldInverse, p = new THREE.Vector3();
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    for (const x of [-bounds.w / 2, bounds.w / 2]) for (const z of [-bounds.d / 2, bounds.d / 2]) for (const y of [0, 3.6]) {
      p.set(bounds.cx + x, y, bounds.cz + z).applyMatrix4(inv);
      x0 = Math.min(x0, p.x); x1 = Math.max(x1, p.x); y0 = Math.min(y0, p.y); y1 = Math.max(y1, p.y);
    }
    const aspect = host.clientWidth / Math.max(1, host.clientHeight);
    const halfH = Math.max((y1 - y0) / 2, (x1 - x0) / 2 / aspect) * 1.08 + 0.6;
    zoom = THREE.MathUtils.clamp(10 / halfH, 0.06, 3.5);
    const right = new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 0), up = new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 1);
    target.addScaledVector(right, (x0 + x1) / 2).addScaledVector(up, (y0 + y1) / 2);
    resize(); placeCam();
  }

  // input: drag to pan, wheel to zoom at the cursor, click to open, right-click for details
  const ray = new THREE.Raycaster(), ndc = new THREE.Vector2();
  let drag = null, hovered = null, dirty = true;
  const canvas = renderer.domElement;
  function pick(e) {
    const r = canvas.getBoundingClientRect();
    ndc.set(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1);
    ray.setFromCamera(ndc, cam);
    const hit = ray.intersectObjects(desks.map(d => d.hit), false)[0];
    return hit ? desks.find(d => d.hit === hit.object) : null;
  }
  function worldAt(e) {
    const r = canvas.getBoundingClientRect();
    ndc.set(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1);
    ray.setFromCamera(ndc, cam);
    const p = new THREE.Vector3();
    ray.ray.intersectPlane(new THREE.Plane(new THREE.Vector3(0, 1, 0), 0), p);
    return p;
  }
  canvas.addEventListener('pointerdown', e => { if (e.button !== 0) return; drag = { x: e.clientX, y: e.clientY, p: worldAt(e), moved: false }; canvas.setPointerCapture(e.pointerId); });
  canvas.addEventListener('pointermove', e => {
    if (drag) {
      if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) > 4) { drag.moved = true; canvas.classList.add('drag'); }
      if (drag.moved) { const p = worldAt(e); target.sub(p.sub(drag.p)); placeCam(); }
      return;
    }
    const d = pick(e);
    setHover(d, e);
  });
  canvas.addEventListener('pointerup', e => {
    canvas.classList.remove('drag');
    if (drag && !drag.moved) { const d = pick(e); if (d) act(d.a, 'focus'); }
    drag = null;
  });
  canvas.addEventListener('pointerleave', () => setHover(null));
  canvas.addEventListener('contextmenu', e => { e.preventDefault(); const d = pick(e); if (d) openDrawer(d.a); });
  canvas.addEventListener('dblclick', e => { const d = pick(e); if (!d) { const p = worldAt(e); target.copy(p); zoom = Math.min(3, zoom * 1.8); resize(); placeCam(); } });
  canvas.addEventListener('wheel', e => {
    e.preventDefault();
    const before = worldAt(e);
    zoom = THREE.MathUtils.clamp(zoom * Math.exp(-e.deltaY * 0.0015), 0.06, 3.5);
    resize(); placeCam();
    const after = worldAt(e);
    target.add(before.sub(after)); placeCam();
  }, { passive: false });
  function setHover(d, e) {
    const tip = $('#tip');
    if (hovered && hovered !== d) { hovered.ring.material.opacity = selected === hovered.a.id ? 0.9 : 0; tags.get(hovered.a.id)?.el.classList.remove('hover'); }
    hovered = d;
    canvas.style.cursor = d ? 'pointer' : 'grab';
    if (!d) { tip.style.display = 'none'; dirty = true; return; }
    d.ring.material.opacity = 0.9;
    const a = d.a, s = a.summary || {};
    tip.innerHTML = `<h4>${esc(a.emoji)} ${esc(a.name)}</h4><div class="meta">${st(a.status).emoji} ${st(a.status).label} · ${esc(a.cli)}${a.model ? ' · ' + esc(model(a.model)) : ''} · ${ago(a.last_activity)}</div>
      <div style="margin:7px 0">📌 <b>${esc(s.job)}</b></div><div>${esc(s.bubble)}</div>
      ${s.needs_you ? `<div class="need" style="margin-top:6px;color:#fecdd3">✋ ${esc(s.needs_you)}</div>` : ''}
      ${(s.done || []).length ? `<div style="margin-top:6px;color:#c7d0de">${s.done.map(x => '✓ ' + esc(x)).join('<br>')}</div>` : ''}
      <div class="ctx">🧠 ${a.context ? `${a.context_pct}% of ${ktok(a.window)}` : 'no session log'} ${adviceBadge(a)}</div>
      <div class="hint">click: open in hivey · right-click: details</div>`;
    tip.style.display = 'block';
    const x = Math.min(e.clientX + 16, innerWidth - 360), y = Math.min(e.clientY + 16, innerHeight - tip.offsetHeight - 10);
    tip.style.left = x + 'px'; tip.style.top = y + 'px';
    tags.get(a.id)?.el.classList.add('hover');
    dirty = true;
    if (!REDUCED) placeLabels();
  }
  // click feedback: a gold ring bursts out, a cyan beam flashes and a light pulses at the agent (1.4 s)
  const flashes = [];
  const beamGeo = new THREE.CylinderGeometry(0.55, 0.55, 7, 28, 1, true);
  const glowMat = color => new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0, blending: THREE.AdditiveBlending, depthWrite: false, toneMapped: false, side: THREE.DoubleSide });
  function flash(id) {
    const d = desks.find(x => x.a.id === id);
    if (!d) return;
    const g = new THREE.Group();
    g.position.copy(d.g.position).add(new THREE.Vector3(0, 0, -0.9 * d.g.scale.x));
    const ring = mesh(G.ring, glowMat('#ffd36a'), 0, 0.06, 0, 1, 1, 1, false); ring.rotation.x = -Math.PI / 2;
    const ring2 = mesh(G.ring, glowMat('#7fe8ff'), 0, 0.07, 0, 1, 1, 1, false); ring2.rotation.x = -Math.PI / 2;
    const beam = mesh(beamGeo, glowMat('#7fe8ff'), 0, 3.5, 0, 1, 1, 1, false);
    const light = new THREE.PointLight('#9fe8ff', 0, 9, 1.4); light.position.y = 2.2;
    g.add(ring, ring2, beam, light); world.add(g);
    flashes.push({ g, ring, ring2, beam, light, t0: performance.now() });
    tags.get(id)?.el.classList.add('hover');
    setTimeout(() => tags.get(id)?.el.classList.remove('hover'), 1600);
    dirty = true;
  }
  function stepFlashes(now) {
    for (let i = flashes.length - 1; i >= 0; i--) {
      const f = flashes[i], p = Math.min(1, (now - f.t0) / 1400), fade = 1 - p;
      const blink = 0.55 + 0.45 * Math.sin(p * Math.PI * 10); // three quick blinks
      f.ring.scale.setScalar(1 + p * 5); f.ring.material.opacity = fade;
      f.ring2.scale.setScalar(1 + Math.max(0, p - 0.15) * 5); f.ring2.material.opacity = p > 0.15 ? fade * 0.8 : 0;
      f.beam.material.opacity = 0.55 * fade * blink; f.beam.scale.set(1 - p * 0.5, 1, 1 - p * 0.5);
      f.light.intensity = 40 * fade * blink;
      if (p >= 1) { world.remove(f.g); [f.ring, f.ring2, f.beam].forEach(m => m.material.dispose()); flashes.splice(i, 1); }
    }
  }
  function markSelected() {
    desks.forEach(d => { d.ring.material.opacity = d.a.id === selected ? 0.9 : 0; tags.get(d.a.id)?.el.classList.toggle('sel', d.a.id === selected); });
    dirty = true;
  }

  // labels follow their agents; level of detail by zoom so hundreds stay readable
  const v = new THREE.Vector3();
  function placeLabels() {
    const w = host.clientWidth, h = host.clientHeight;
    const px = zoom * h / 20; // pixels per world unit
    labels.className = px < 7 ? 'lod-tiny' : '';
    // most urgent first; a label that would cover one already placed drops its bubble, then its plate
    const placed = [];
    const hits = (x0, y0, x1, y1) => placed.some(r => x0 < r[2] && x1 > r[0] && y0 < r[3] && y1 > r[1]);
    const first = ordered.filter(t => t.d.a.id === selected || t.el.classList.contains('hover'));
    for (const t of [...first, ...ordered.filter(t => !first.includes(t))]) {
      const { el, d } = t;
      v.copy(d.anchor).project(cam);
      const x = (v.x + 1) / 2 * w, y = (1 - v.y) / 2 * h;
      if (x < -200 || x > w + 200 || y < -100 || y > h + 300) { el.style.display = 'none'; continue; }
      const forced = first.includes(t);
      const pw = t.pw / 2, bw = Math.max(t.bw, t.pw) / 2;
      let mode = 'full';
      if (!forced && (labels.className === 'lod-tiny' || hits(x - bw, y - t.ph - t.bh, x + bw, y))) mode = hits(x - pw, y - t.ph, x + pw, y) ? 'none' : 'plate';
      if (mode === 'none' && d.a.status === 'blocked') mode = 'plate';
      el.style.display = mode === 'none' ? 'none' : 'flex';
      el.classList.toggle('nobubble', mode === 'plate');
      if (mode === 'none') continue;
      placed.push(mode === 'full' ? [x - bw, y - t.ph - t.bh, x + bw, y] : [x - pw, y - t.ph, x + pw, y]);
      el.style.transform = `translate(${x}px, ${y}px) translate(-50%, -100%)`;
    }
    roomTags.forEach(({ el, p }) => { v.copy(p).project(cam); el.style.transform = `translate(${(v.x + 1) / 2 * w}px, ${(1 - v.y) / 2 * h}px)`; });
  }
  // the hover class keeps a bubble visible while zoomed out
  const style = document.createElement('style');
  style.textContent = '.tag.nobubble .bubble{display:none}.tag.hover .bubble,.tag.sel .bubble{display:block!important}.tag.hover,.tag.sel{display:flex!important;z-index:2}';
  document.head.appendChild(style);

  // animation: only working agents (and their team) move; a waiting agent waves; the rest stand still
  let last = 0, lastCode = 0;
  function frame(t) {
    requestAnimationFrame(frame);
    if (document.hidden || ui.view !== 'office') return;
    const moving = flashes.length > 0 || (!REDUCED && desks.some(d => d.a.status === 'working' || d.a.status === 'blocked' || d.team.some(m => m.status === 'working')));
    const dt = Math.min(0.1, (t - (frame.prev || t)) / 1000); frame.prev = t;
    if (!moving && !dirty) return;
    if (moving && t - last < 33) return; // ~30 fps is plenty
    last = t;
    const s = t / 1000;
    if (moving) {
      const pl = ship.planet(); if (pl) pl.rotation.y = s * 0.4;
      if (t - lastCode > 110) { drawCode(); lastCode = t; }
      for (const d of desks) {
        const b = d.bot, ph = d.ph;
        if (b.tick) { b.tick(dt, s); d.team.forEach(m => m.tick?.(dt, s)); if (d.holo) d.holo.position.y = 2.15 + Math.sin(s * 2 + ph) * 0.04; continue; }
        if (d.a.status === 'working') {
          b.body.position.y = Math.sin(s * 7 + ph) * 0.025;
          b.arms[0].rotation.x = -1.15 + Math.sin(s * 16 + ph) * 0.28;
          b.arms[1].rotation.x = -1.15 + Math.sin(s * 16 + ph + Math.PI) * 0.28;
          b.head.rotation.y = Math.sin(s * 1.4 + ph) * 0.3;
          b.head.rotation.x = -0.08 + Math.sin(s * 2.1 + ph) * 0.06;
          if (b.scan) b.scan.position.x = Math.sin(s * 3.2 + ph) * 0.13;
          if (b.hover) b.bot.position.y = 0.2 + Math.sin(s * 2.4 + ph) * 0.05;
          b.wings.forEach((w, i) => w.rotation.y = (i ? -1 : 1) * (0.25 + Math.sin(s * 38 + ph) * 0.35));
          if (d.holo) d.holo.position.y = 1.95 + Math.sin(s * 2 + ph) * 0.05;
        } else if (d.a.status === 'blocked') {
          b.arms[1].rotation.z = 0.3 + Math.sin(s * 6 + ph) * 0.35;
          b.body.position.y = Math.abs(Math.sin(s * 3 + ph)) * 0.06;
          if (d.sign) d.sign.scale.setScalar(0.5 + Math.sin(s * 5) * 0.07);
        }
        for (const m of d.team) {
          if (m.status !== 'working') continue;
          m.body.position.y = Math.abs(Math.sin(s * 5 + m.ph)) * 0.18;
          m.wings.forEach((w, i) => w.rotation.y = (i ? -1 : 1) * (0.25 + Math.sin(s * 40 + m.ph) * 0.4));
        }
      }
    }
    if (flashes.length) stepFlashes(performance.now());
    if (ui.toon) outline.render(scene, cam); else composer.render();
    placeLabels();
    dirty = false;
  }
  new ResizeObserver(() => { resize(); }).observe(host);
  resize(); placeCam();
  requestAnimationFrame(frame);
  return { ok: true, build, markSelected, resize, fit, flash, ready: ship.load() };
})();

// ---------------------------------------------------------------- views, polling, keys
let lastGenerated = null, refreshing = false;
function renderAll() {
  if (!data) return;
  agents = data.agents || [];
  renderHeader();
  renderInbox();
  document.querySelectorAll('#views button').forEach(b => b.classList.toggle('on', b.dataset.v === ui.view));
  $('#office').style.display = ui.view === 'office' ? '' : 'none';
  $('#board').style.display = ui.view === 'board' ? 'block' : 'none';
  $('#list').style.display = ui.view === 'list' ? 'block' : 'none';
  if (ui.view === 'office') { office.build(agents.filter(visible)); office.resize(); }
  if (ui.view === 'board') renderBoard();
  if (ui.view === 'list') renderList();
  if (selected) { const a = agents.find(x => x.id === selected); if (a && $('#drawer').classList.contains('open')) openDrawer(a); }
}
async function poll() {
  try {
    if (office.ready) await office.ready.catch(() => {});
    const r = await fetch('./office.json?ts=' + Date.now(), { cache: 'no-store' });
    const j = await r.json();
    if (j.generated !== lastGenerated) {
      lastGenerated = j.generated; data = j;
      if (refreshing && Date.parse(j.generated) > refreshing) { refreshing = false; $('#refresh').innerHTML = '⟳'; toast('Office updated'); }
      renderAll();
    }
  } catch (e) { console.error('hivey office:', e); $('#sub').textContent = 'cannot read office.json: is the office server running? (hivey menu → agent office)'; }
}
async function refreshNow() {
  if (refreshing) return;
  refreshing = Date.now();
  $('#refresh').innerHTML = '<span class="spin">⟳</span>';
  tickSub();
  try { await fetch('/hv-office-refresh', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' }); }
  catch (e) { toast('The office server is not answering'); refreshing = false; $('#refresh').innerHTML = '⟳'; }
  setTimeout(() => { if (refreshing) { refreshing = false; $('#refresh').innerHTML = '⟳'; } }, 180000);
}
document.querySelectorAll('#views button').forEach(b => b.onclick = () => { ui.view = b.dataset.v; save(); renderAll(); });
$('#refresh').onclick = refreshNow;
// background music: a MusicGen space ambient loop, off until you turn it on (browsers block autoplay)
const bgm = $('#bgm');
bgm.volume = 0.35;
function setMusic(on) {
  ui.music = on; save();
  $('#music').textContent = on ? '🎵' : '🔇';
  $('#music').title = on ? 'Music on: click to mute (m)' : 'Background music on/off (m)';
  if (on) bgm.play().catch(() => { $('#music').textContent = '🔇'; toast('Click 🔇 to start the music'); }); else bgm.pause();
}
$('#music').onclick = () => setMusic(!ui.music);
if (ui.music) addEventListener('pointerdown', () => { if (ui.music && bgm.paused) setMusic(true); }, { once: true });
$('#music').textContent = ui.music ? '🎵' : '🔇';
// only while the page is in front of you: pause when you switch to the terminal or another app or tab
const inFront = () => !document.hidden && document.hasFocus();
function followFocus() { if (!ui.music) return; if (inFront()) bgm.play().catch(() => {}); else bgm.pause(); }
addEventListener('blur', followFocus); addEventListener('focus', followFocus);
document.addEventListener('visibilitychange', followFocus);
$('#fit').onclick = () => office.fit();
function toggleStyle() { ui.toon = !ui.toon; save(); $('#style').textContent = ui.toon ? '🎨 comic' : '✨ HD'; renderAll(); }
$('#style').onclick = toggleStyle;
$('#style').textContent = ui.toon ? '🎨 comic' : '✨ HD';
let qTimer;
$('#q').addEventListener('input', () => { clearTimeout(qTimer); qTimer = setTimeout(renderAll, 150); });
addEventListener('keydown', e => {
  if (e.target.matches('input')) { if (e.key === 'Escape') { e.target.value = ''; e.target.blur(); renderAll(); } return; }
  if (e.key === '/') { e.preventDefault(); $('#q').focus(); }
  else if (e.key === 'Escape') closeDrawer();
  else if (e.key === '1' || e.key === '2' || e.key === '3') { ui.view = ['office', 'board', 'list'][+e.key - 1]; save(); renderAll(); }
  else if (e.key === 'r') refreshNow();
  else if (e.key === 'f') office.fit();
  else if (e.key === 't') toggleStyle();
  else if (e.key === 'm') setMusic(!ui.music);
});
if (!office.ok && ui.view === 'office') ui.view = 'board';
if (!office.ok) document.querySelector('#views [data-v="office"]').disabled = true;
setInterval(poll, 15000);
setInterval(tickSub, 1000);
poll();
