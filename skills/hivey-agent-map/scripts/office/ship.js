// hivey office, spacecraft edition: the deck of a ship, its workstations and its crew of mini robots.
// Everything is built from primitives and canvas textures (no image assets): physically based shells,
// glass visors and emissive LEDs that the bloom pass makes glow.
import * as THREE from './three.module.min.js';
import { GLTFLoader } from './loaders/GLTFLoader.js';
import * as SkeletonUtils from './utils/SkeletonUtils.js';

// CC0 models (see models/LICENSE-*): the animated robot (Tomás Laulhé / Quaternius) and Kenney's
// Space Kit and Space Station Kit. Anything that fails to load falls back to the built-in shapes.
const FILES = {
  robot: 'models/robot.glb', desk: 'models/desk_computer.glb', chair: 'models/desk_chairArms.glb',
  generator: 'models/machine_generator.glb', generatorL: 'models/machine_generatorLarge.glb', wireless: 'models/machine_wireless.glb',
  dish: 'models/satelliteDish_detailed.glb', dishL: 'models/satelliteDish_large.glb', speeder: 'models/craft_speederA.glb',
  racer: 'models/craft_racer.glb', cargo: 'models/craft_cargoA.glb', barrelsRail: 'models/barrels_rail.glb', barrels: 'models/barrels.glb',
  astroA: 'models/astronautA.glb', astroB: 'models/astronautB.glb',
  wallDisplay: 'models/station/display-wall-wide.glb', console: 'models/station/computer-system.glb', consoleWide: 'models/station/computer-wide.glb',
  crateTall: 'models/station/container-tall.glb', crateWide: 'models/station/container-wide.glb', crateFlat: 'models/station/container-flat.glb',
  lounger: 'models/station/chair-cushion-headrest.glb',
};

export function createShip(opts) {
  const { mesh, hue, st } = opts;
  const MODELS = {};
  let robotScale = 1;
  async function load() {
    const loader = new GLTFLoader();
    await Promise.all(Object.entries(FILES).map(([k, url]) => loader.loadAsync(url).then(g => { MODELS[k] = g; }).catch(e => console.warn('hivey office: model', k, e))));
    const c = new THREE.Vector3();
    for (const [k, g] of Object.entries(MODELS)) {
      g.scene.updateMatrixWorld(true);
      const b = new THREE.Box3().setFromObject(g.scene, true); // precise: counts skinning
      if (k === 'robot') { robotScale = 1.8 / Math.max(0.001, b.max.y - b.min.y); continue; }
      // Kenney models sit off their origin: re-centre each on its base so it lands where it is placed
      b.getCenter(c); g.scene.position.set(-c.x, -b.min.y, -c.z);
      g.pivot = new THREE.Group(); g.pivot.add(g.scene);
      // their light metals blow out under bloom: tone them down once (materials are shared by clones)
      g.scene.traverse(m => { if (!m.isMesh) return; [].concat(m.material).forEach(x => {
        if (x.userData.toned) return; x.userData.toned = true;
        x.color?.multiplyScalar(x.map ? 0.7 : 0.42); x.metalness = Math.max(x.metalness ?? 0, 0.5); x.roughness = 0.4; x.envMapIntensity = 0.45;
      }); });
    }
  }
  function model(k, scale = 1) {
    const g = MODELS[k]; if (!g?.pivot) return null;
    const o = g.pivot.clone(true); o.scale.setScalar(scale);
    o.traverse(m => { if (m.isMesh) { m.castShadow = true; m.receiveShadow = true; } });
    return o;
  }
  function place(grp, k, scale, x, y, z, ry = 0) { const o = model(k, scale); if (!o) return null; o.position.set(x, y, z); o.rotation.y = ry; grp.add(o); return o; }
  // ---------------------------------------------------------------- helpers
  let seedN = 11;
  const rnd = () => (seedN = (seedN * 16807) % 2147483647) / 2147483647;
  const tex = (w, h, draw, rx = 1, ry = 1) => {
    const c = document.createElement('canvas'); c.width = w; c.height = h; draw(c.getContext('2d'), w, h);
    const t = new THREE.CanvasTexture(c); t.colorSpace = THREE.SRGBColorSpace; t.anisotropy = 8;
    if (rx !== 1 || ry !== 1) { t.wrapS = t.wrapT = THREE.RepeatWrapping; t.repeat.set(rx, ry); }
    return t;
  };
  const cache = {};
  const once = (k, make) => cache[k] || (cache[k] = make());
  const put = (grp, geo, mat, x, y, z, sx = 1, sy = 1, sz = 1, ry = 0, shadow = true) => {
    const m = mesh(geo, mat, x, y, z, sx, sy, sz, shadow); m.rotation.y = ry; grp.add(m); return m;
  };
  const NO_LINE = { visible: false };
  const flat = m => { m.userData.outlineParameters = NO_LINE; return m; };

  const U = {
    box: new THREE.BoxGeometry(1, 1, 1), cyl: new THREE.CylinderGeometry(0.5, 0.5, 1, 28), sph: new THREE.SphereGeometry(0.5, 28, 20),
    plane: new THREE.PlaneGeometry(1, 1), circ: new THREE.CircleGeometry(0.5, 48), ring: new THREE.RingGeometry(0.42, 0.5, 48),
    torus: new THREE.TorusGeometry(0.5, 0.05, 10, 48), cap: new THREE.CapsuleGeometry(0.5, 1, 8, 20), ico: new THREE.IcosahedronGeometry(0.5, 0),
    cone: new THREE.ConeGeometry(0.5, 1, 28, 1, true), wire: new THREE.IcosahedronGeometry(0.5, 2),
    visor: new THREE.SphereGeometry(0.5, 32, 16, Math.PI / 2 - 1.0, 2.0, Math.PI * 0.3, Math.PI * 0.36),
  };

  // ---------------------------------------------------------------- materials
  const SHELLS = ['#e9edf2', '#3a3f47', '#5b6470', '#1c1f24', '#a3abb5', '#c8b69a', '#2d3f66', '#7a2e2e'];
  const shell = c => once('shell' + c, () => new THREE.MeshPhysicalMaterial({ color: c, metalness: 0.35, roughness: 0.38, clearcoat: 0.5, clearcoatRoughness: 0.25, envMapIntensity: 0.7 }));
  const joint = once('joint', () => new THREE.MeshStandardMaterial({ color: '#15171b', roughness: 0.55, metalness: 0.4 }));
  const glass = once('glass', () => new THREE.MeshPhysicalMaterial({ color: '#020306', metalness: 0.1, roughness: 0.12, clearcoat: 1, clearcoatRoughness: 0.05, envMapIntensity: 0.35 }));
  const led = (color, k = 3) => once(`led${color}${k}`, () => new THREE.MeshStandardMaterial({ color, emissive: color, emissiveIntensity: k, toneMapped: false }));
  const statusLed = s => led(st(s).color, s === 'idle' ? 1.1 : s === 'working' ? 3.0 : 2.4);
  const accent = h => led(`hsl(${h}, 85%, 58%)`, 1.3);
  const M = {
    deck: once('deck', () => new THREE.MeshStandardMaterial({ map: deckTex(), roughness: 0.42, metalness: 0.65 })),
    hull: once('hull', () => new THREE.MeshStandardMaterial({ map: hullTex(), roughness: 0.5, metalness: 0.6 })),
    rib: once('rib', () => new THREE.MeshStandardMaterial({ color: '#2a3039', roughness: 0.35, metalness: 0.8 })),
    dark: once('dark', () => new THREE.MeshStandardMaterial({ color: '#1a1e25', roughness: 0.35, metalness: 0.6 })),
    slab: once('slab', () => new THREE.MeshPhysicalMaterial({ color: '#20252e', roughness: 0.25, metalness: 0.5, clearcoat: 0.8 })),
    white: once('white', () => new THREE.MeshPhysicalMaterial({ color: '#e7ebf0', roughness: 0.3, metalness: 0.1, clearcoat: 0.6 })),
    leather: once('leather', () => new THREE.MeshStandardMaterial({ color: '#2a2f3a', roughness: 0.6, metalness: 0.05 })),
    pane: flat(once('pane', () => new THREE.MeshPhysicalMaterial({ color: '#9fd8ff', roughness: 0.05, metalness: 0, transparent: true, opacity: 0.16, depthWrite: false }))),
    leaf: once('leaf', () => new THREE.MeshStandardMaterial({ color: '#36b26a', roughness: 0.5, flatShading: true })),
    hazard: flat(once('hazard', () => new THREE.MeshStandardMaterial({ map: hazardTex(), roughness: 0.6, metalness: 0.3 }))),
    space: flat(once('space', () => new THREE.MeshBasicMaterial({ map: spaceTex(1024, 512, true), toneMapped: false }))),
  };

  // ---------------------------------------------------------------- textures
  function deckTex() { // brushed metal deck plates with seams, rivets and wear
    return tex(512, 512, (g, w, h) => {
      g.fillStyle = '#2b313b'; g.fillRect(0, 0, w, h);
      for (let i = 0; i < 9000; i++) { g.fillStyle = `rgba(255,255,255,${rnd() * 0.035})`; g.fillRect(rnd() * w, rnd() * h, 1 + rnd() * 10, 1); }
      for (let y = 0; y < 4; y++) for (let x = 0; x < 4; x++) {
        const px = x * 128, py = y * 128, l = 18 + rnd() * 5;
        g.fillStyle = `hsla(215, 12%, ${l}%, .55)`; g.fillRect(px + 3, py + 3, 122, 122);
        g.strokeStyle = 'rgba(0,0,0,.65)'; g.lineWidth = 3; g.strokeRect(px + 1.5, py + 1.5, 125, 125);
        g.strokeStyle = 'rgba(255,255,255,.07)'; g.lineWidth = 1; g.strokeRect(px + 5, py + 5, 118, 118);
        g.fillStyle = 'rgba(200,210,225,.35)'; [[10, 10], [118, 10], [10, 118], [118, 118]].forEach(([a, b]) => { g.beginPath(); g.arc(px + a, py + b, 2.2, 0, 7); g.fill(); });
        if ((x + y) % 3 === 0) { g.strokeStyle = 'rgba(0,0,0,.35)'; for (let k = 0; k < 6; k++) { g.beginPath(); g.moveTo(px + 30 + k * 12, py + 40); g.lineTo(px + 30 + k * 12, py + 88); g.stroke(); } }
      }
    });
  }
  function hullTex() { // wall panels
    return tex(512, 512, (g, w, h) => {
      g.fillStyle = '#353c47'; g.fillRect(0, 0, w, h);
      for (let i = 0; i < 4000; i++) { g.fillStyle = `rgba(255,255,255,${rnd() * 0.03})`; g.fillRect(rnd() * w, rnd() * h, 1, 1 + rnd() * 8); }
      g.strokeStyle = 'rgba(0,0,0,.55)'; g.lineWidth = 3;
      [[0, 0, 256, 170], [256, 0, 256, 170], [0, 170, 170, 342], [170, 170, 342, 200], [170, 370, 342, 142]].forEach(r => g.strokeRect(...r));
      g.fillStyle = 'rgba(0,0,0,.25)'; g.fillRect(200, 200, 280, 26); g.fillRect(30, 220, 100, 240);
      g.fillStyle = 'rgba(255,200,60,.5)'; g.font = 'bold 18px monospace'; g.fillText('HV-01  DECK 3', 210, 219);
    });
  }
  function hazardTex() {
    return tex(256, 32, (g, w, h) => { g.fillStyle = '#e8b511'; g.fillRect(0, 0, w, h); g.fillStyle = '#16181c'; for (let x = -32; x < w; x += 32) { g.beginPath(); g.moveTo(x, h); g.lineTo(x + 16, 0); g.lineTo(x + 32, 0); g.lineTo(x + 16, h); g.fill(); } }, 1, 1);
  }
  function spaceTex(w, h, planet) { // stars, a nebula and (in viewports) a planet with its atmosphere
    return tex(w, h, (g) => {
      const bg = g.createLinearGradient(0, 0, w, h); bg.addColorStop(0, '#02030a'); bg.addColorStop(1, '#070b1d');
      g.fillStyle = bg; g.fillRect(0, 0, w, h);
      [['rgba(120,60,200,.35)', .25, .35, .45], ['rgba(20,160,200,.25)', .7, .6, .4], ['rgba(220,60,120,.18)', .5, .2, .3]].forEach(([c, x, y, r]) => {
        const n = g.createRadialGradient(x * w, y * h, 0, x * w, y * h, r * w); n.addColorStop(0, c); n.addColorStop(1, 'rgba(0,0,0,0)'); g.fillStyle = n; g.fillRect(0, 0, w, h);
      });
      for (let i = 0; i < w * h / 900; i++) { const b = rnd(); g.fillStyle = `rgba(${200 + b * 55},${210 + b * 45},255,${0.3 + b * 0.7})`; const s = b > 0.97 ? 2.4 : b > 0.85 ? 1.6 : 1; g.fillRect(rnd() * w, rnd() * h, s, s); }
      if (!planet) return;
      const px = w * 0.72, py = h * 0.78, pr = h * 0.42;
      const atm = g.createRadialGradient(px, py, pr * 0.9, px, py, pr * 1.25); atm.addColorStop(0, 'rgba(90,180,255,.55)'); atm.addColorStop(1, 'rgba(90,180,255,0)');
      g.fillStyle = atm; g.beginPath(); g.arc(px, py, pr * 1.25, 0, 7); g.fill();
      const pg = g.createRadialGradient(px - pr * 0.4, py - pr * 0.5, pr * 0.1, px, py, pr); pg.addColorStop(0, '#5fb3ff'); pg.addColorStop(0.5, '#1f5fa8'); pg.addColorStop(1, '#06162e');
      g.fillStyle = pg; g.beginPath(); g.arc(px, py, pr, 0, 7); g.fill();
      g.save(); g.beginPath(); g.arc(px, py, pr, 0, 7); g.clip();
      g.fillStyle = 'rgba(255,255,255,.18)'; for (let i = 0; i < 9; i++) { g.beginPath(); g.ellipse(px - pr + rnd() * 2 * pr, py - pr + rnd() * 2 * pr, 40 + rnd() * 90, 6 + rnd() * 10, -0.2, 0, 7); g.fill(); }
      g.fillStyle = 'rgba(40,140,80,.35)'; for (let i = 0; i < 5; i++) { g.beginPath(); g.ellipse(px - pr * .6 + rnd() * pr, py - pr * .5 + rnd() * pr, 30 + rnd() * 50, 18 + rnd() * 30, rnd(), 0, 7); g.fill(); }
      const sh = g.createLinearGradient(px - pr, py - pr, px + pr, py + pr); sh.addColorStop(0.35, 'rgba(0,0,0,0)'); sh.addColorStop(1, 'rgba(0,0,0,.8)');
      g.fillStyle = sh; g.fillRect(px - pr, py - pr, pr * 2, pr * 2); g.restore();
      g.fillStyle = '#c9ccd6'; g.beginPath(); g.arc(w * 0.18, h * 0.22, h * 0.05, 0, 7); g.fill(); // moon
    });
  }
  const screenTex = kind => once('screen' + kind, () => tex(256, 160, (g, w, h) => { // wall displays
    g.fillStyle = '#04121c'; g.fillRect(0, 0, w, h);
    g.strokeStyle = '#2fd3ff'; g.fillStyle = '#2fd3ff'; g.lineWidth = 2; g.strokeRect(4, 4, w - 8, h - 8);
    g.font = 'bold 12px monospace';
    if (kind === 0) { g.fillText('NAV // SECTOR 7G', 12, 22); for (let r = 20; r < 70; r += 16) { g.beginPath(); g.arc(128, 92, r, 0, 7); g.stroke(); } g.beginPath(); g.moveTo(128, 92); g.lineTo(190, 60); g.stroke(); g.fillStyle = '#ffcf3a'; g.fillRect(160, 70, 5, 5); g.fillRect(100, 110, 4, 4); }
    else if (kind === 1) { g.fillText('POWER  98.2%', 12, 22); for (let i = 0; i < 12; i++) { const bh = 20 + rnd() * 90; g.fillStyle = i % 4 ? '#2fd3ff' : '#ffcf3a'; g.fillRect(16 + i * 19, 145 - bh, 12, bh); } }
    else if (kind === 2) { g.fillText('HIVEY-1  CREW', 12, 22); g.beginPath(); g.moveTo(30, 90); g.lineTo(90, 60); g.lineTo(200, 60); g.lineTo(230, 90); g.lineTo(200, 120); g.lineTo(90, 120); g.closePath(); g.stroke(); g.fillStyle = '#ff5d7a'; g.fillRect(120, 85, 8, 8); g.fillStyle = '#4ade80'; g.fillRect(150, 75, 8, 8); g.fillRect(170, 98, 8, 8); }
    else { g.fillText('TELEMETRY', 12, 22); g.beginPath(); for (let x = 12; x < w - 12; x += 4) g.lineTo(x, 95 + Math.sin(x / 14) * 25 + rnd() * 10); g.stroke(); g.strokeStyle = '#ffcf3a'; g.beginPath(); for (let x = 12; x < w - 12; x += 4) g.lineTo(x, 120 + Math.cos(x / 22) * 12); g.stroke(); }
  }));
  const screenMat = k => flat(once('screenMat' + k, () => new THREE.MeshStandardMaterial({ color: '#000', emissive: '#fff', emissiveMap: screenTex(k), emissiveIntensity: 1.4, toneMapped: false })));
  const rackTex = once('rackTex', () => tex(128, 256, (g, w, h) => {
    g.fillStyle = '#06080c'; g.fillRect(0, 0, w, h);
    for (let y = 8; y < h - 8; y += 22) { g.fillStyle = '#12161d'; g.fillRect(6, y, w - 12, 18); for (let x = 12; x < w - 12; x += 9) if (rnd() < 0.55) { g.fillStyle = rnd() < 0.8 ? '#35e0ff' : rnd() < 0.5 ? '#4ade80' : '#ffb020'; g.fillRect(x, y + 7, 4, 4); } }
  }));
  const holoTex = s => once('holo' + s, () => tex(256, 160, (g, w, h) => { // a desk screen when not working
    const c = st(s).color;
    g.fillStyle = 'rgba(4,14,24,.85)'; g.fillRect(0, 0, w, h); g.strokeStyle = c; g.lineWidth = 3; g.strokeRect(3, 3, w - 6, h - 6);
    g.fillStyle = c; g.globalAlpha = 0.85; for (let i = 0; i < 6; i++) g.fillRect(16, 24 + i * 20, 60 + ((i * 53) % 140), 7);
    g.globalAlpha = 1; g.font = 'bold 18px monospace'; g.fillText(s.toUpperCase(), w - 110, h - 16);
  }));

  // ---------------------------------------------------------------- the crew: mini robots
  // A hash of the name picks shell, head, eyes, antenna and whether it hovers; shoulder pods and
  // chest strip glow in its team colour, its eyes in its status colour.
  function glbRobot(h, status, small, seed) {
    const r = (k, n) => hue(seed + '|' + k) % n;
    const root = new THREE.Group(), o = SkeletonUtils.clone(MODELS.robot.scene);
    o.scale.setScalar(robotScale * (small ? 0.5 : 1));
    const shellC = SHELLS[r('shell', SHELLS.length)];
    o.traverse(m => {
      if (!m.isMesh) return;
      m.castShadow = m.receiveShadow = true; m.frustumCulled = false;
      const one = x => {
        const c = x.clone();
        if (x.name === 'Main') { c.color.set(shellC); c.metalness = 0.55; c.roughness = 0.32; }
        else if (x.name === 'Grey') { c.color.set('#262b33'); c.metalness = 0.6; c.roughness = 0.4; c.emissive = new THREE.Color(`hsl(${h}, 85%, 50%)`); c.emissiveIntensity = 0.45; }
        else { c.metalness = 0.3; c.roughness = 0.18; }
        return c;
      };
      m.material = Array.isArray(m.material) ? m.material.map(one) : one(m.material);
    });
    root.add(o);
    const mixer = new THREE.AnimationMixer(o), clips = MODELS.robot.animations;
    const act = n => { const c = clips.find(x => x.name === n); return c ? mixer.clipAction(c) : null; };
    const still = (n, at) => { const a = act(n); if (!a) return; a.play(); a.time = a.getClip().duration * at; a.paused = true; mixer.update(0); };
    let idle = null, nod = null;
    if (status === 'working') { idle = act('Idle'); idle?.play(); nod = act('Yes'); if (nod) { nod.setLoop(THREE.LoopOnce); nod.clampWhenFinished = false; } }
    else if (status === 'blocked') act('Wave')?.play();
    else if (status === 'done') still('ThumbsUp', 0.45);
    else if (status === 'idle' && !small) still('Sitting', 0.98);
    else still('Idle', 0);
    const animated = status === 'working' || status === 'blocked';
    let nextNod = 2 + Math.random() * 4;
    // working robots idle and nod every few seconds, as if confirming each step
    const tick = (dt, t) => {
      if (!animated) return;
      if (nod && idle && t > nextNod) { nod.reset().fadeIn(0.25).play(); idle.fadeOut(0.25); nextNod = t + 4 + Math.random() * 4; setTimeout(() => { idle.reset().fadeIn(0.3).play(); nod.fadeOut(0.3); }, nod.getClip().duration * 1000); }
      mixer.update(dt);
    };
    return { bot: root, body: new THREE.Group(), head: new THREE.Group(), arms: [], wings: [], scan: null, hover: false, glb: true, tick, seated: status === 'idle' && !small };
  }

  function makeRobot(h, status, small = false, seed = '') {
    if (MODELS.robot) return glbRobot(h, status, small, seed);
    const r = (k, n) => hue(seed + '|' + k) % n;
    const sh = shell(SHELLS[r('shell', SHELLS.length)]);
    const trim = shell(SHELLS[(r('shell', SHELLS.length) + 3) % SHELLS.length]);
    const eye = statusLed(status), acc = accent(h);
    const hover = r('hover', 4) === 0;
    const bot = new THREE.Group(), body = new THREE.Group(); bot.add(body);
    // torso
    put(body, U.cap, sh, 0, 0.82, 0, 0.6, 0.32, 0.48);
    put(body, U.box, glass, 0, 0.86, 0.2, 0.34, 0.24, 0.06);
    put(body, U.box, acc, 0, 0.8, 0.235, 0.22, 0.03, 0.01, 0, false);
    put(body, U.cyl, joint, 0, 0.5, 0, 0.42, 0.12, 0.34);
    if (hover) { // thruster skirt instead of legs
      put(body, U.cyl, trim, 0, 0.36, 0, 0.36, 0.2, 0.3);
      const ring = put(body, U.torus, acc, 0, 0.24, 0, 0.5, 0.5, 0.5, 0, false); ring.rotation.x = Math.PI / 2;
      put(body, U.circ, led('#8be9ff', 4), 0, 0.23, 0, 0.3, 0.3, 1, 0, false).rotation.x = Math.PI / 2;
    } else { // seated legs
      [-1, 1].forEach(sx => {
        const th = put(body, U.cap, sh, 0.14 * sx, 0.46, 0.2, 0.17, 0.18, 0.17); th.rotation.x = Math.PI / 2;
        put(body, U.sph, joint, 0.14 * sx, 0.46, 0.42, 0.15, 0.15, 0.15);
        put(body, U.cap, trim, 0.14 * sx, 0.22, 0.44, 0.15, 0.2, 0.15);
        put(body, U.box, joint, 0.14 * sx, 0.04, 0.5, 0.16, 0.07, 0.24);
      });
    }
    // neck and head
    put(body, U.cyl, joint, 0, 1.1, 0, 0.14, 0.12, 0.14);
    const head = new THREE.Group(); head.position.set(0, 1.36, 0); body.add(head);
    const pill = r('head', 3);
    if (pill === 0) { const hh = put(head, U.cap, sh, 0, 0, 0, 0.5, 0.42, 0.5); hh.rotation.z = Math.PI / 2; }
    else put(head, U.sph, sh, 0, 0, 0, pill === 1 ? 0.78 : 0.7, pill === 1 ? 0.6 : 0.68, 0.66);
    put(head, U.visor, glass, 0, 0.01, 0.0, 0.8, 0.72, 0.72);
    const eyes = r('eyes', 3);
    let scan = null;
    if (eyes === 0) [-1, 1].forEach(sx => { const e = put(head, U.cap, eye, 0.11 * sx, 0.02, 0.262, 0.075, 0.07, 0.04, 0, false); e.rotation.z = Math.PI / 2; });
    else if (eyes === 1) { put(head, U.box, led('#0a0f16', 0.2), 0, 0.02, 0.258, 0.38, 0.08, 0.02, 0, false); scan = put(head, U.box, eye, 0, 0.02, 0.268, 0.14, 0.07, 0.02, 0, false); }
    else { const ringE = put(head, U.torus, eye, 0, 0.02, 0.262, 0.2, 0.2, 0.2, 0, false); put(head, U.sph, eye, 0, 0.02, 0.258, 0.07, 0.07, 0.04, 0, false); ringE.rotation.x = 0; }
    [-1, 1].forEach(sx => { // ear pods
      const pod = put(head, U.cyl, trim, 0.37 * sx, 0, 0, 0.17, 0.08, 0.17); pod.rotation.z = Math.PI / 2;
      const ring = put(head, U.torus, acc, 0.415 * sx, 0, 0, 0.13, 0.13, 0.13, 0, false); ring.rotation.y = Math.PI / 2;
    });
    if (r('antenna', 2) === 0) { put(head, U.cyl, joint, 0.16, 0.34, -0.05, 0.02, 0.24, 0.02); put(head, U.sph, eye, 0.16, 0.47, -0.05, 0.06, 0.06, 0.06, 0, false); }
    // arms: shoulder, upper arm, elbow, forearm, hand
    const arms = [-1, 1].map(sx => {
      const pivot = new THREE.Group(); pivot.position.set(0.37 * sx, 0.98, 0); body.add(pivot);
      put(pivot, U.sph, trim, 0, 0, 0, 0.17, 0.17, 0.17);
      put(pivot, U.cap, sh, 0, -0.16, 0, 0.1, 0.12, 0.1);
      put(pivot, U.sph, joint, 0, -0.29, 0, 0.09, 0.09, 0.09);
      put(pivot, U.cap, sh, 0, -0.41, 0, 0.09, 0.1, 0.09);
      put(pivot, U.sph, joint, 0, -0.53, 0, 0.1, 0.08, 0.1);
      pivot.rotation.x = -1.0; pivot.rotation.z = 0.06 * sx;
      return pivot;
    });
    if (hover) bot.position.y = 0.15;
    head.rotation.x = -0.12;
    bot.scale.setScalar(small ? 0.5 : 1.15);
    return { bot, body, head, arms, wings: [], scan, hover };
  }

  // ---------------------------------------------------------------- a workstation
  function makeStation(g, a, h, workMat) {
    if (MODELS.desk) {
      place(g, 'desk', 3.1, 0, 0, 0.05, 0);
      put(g, U.box, accent(h), 0, 0.03, 0.5, 2.1, 0.03, 0.02, 0, false);
      const scr = put(g, U.plane, a.status === 'working' ? workMat : holoMat(a.status), 0, 1.75, 0.3, 1.2, 0.62, 1, Math.PI, false);
      scr.visible = a.status === 'working' || a.status === 'blocked';
      const chair = new THREE.Group(); chair.position.set(0, 0, -0.95); g.add(chair);
      place(chair, 'chair', 3.2, 0, 0, 0, Math.PI);
      if (a.status !== 'idle') { chair.position.set(0.95, 0, -1.1); chair.rotation.y = -0.6; }
      return { screen: scr, chair };
    }
    put(g, U.box, M.slab, 0, 0.78, 0, 2.1, 0.07, 1.0);
    put(g, U.box, accent(h), 0, 0.765, 0.505, 2.1, 0.025, 0.012, 0, false);
    put(g, U.box, M.dark, 0, 0.39, -0.1, 1.6, 0.74, 0.12);
    put(g, U.box, M.dark, 0, 0.02, 0, 1.9, 0.04, 0.8);
    // holo keyboard and a floating screen that faces the robot
    put(g, U.plane, flat(led(st(a.status).color, 0.9)), 0, 0.82, -0.2, 0.7, 0.22, 1, 0, false).rotation.x = -Math.PI / 2;
    const scr = put(g, U.plane, a.status === 'working' ? workMat : holoMat(a.status), 0, 1.38, 0.05, 1.15, 0.66, 1, Math.PI, false);
    scr.rotation.x = 0.12;
    put(g, U.box, M.dark, 0, 0.95, 0.12, 0.08, 0.3, 0.05);
    // chair (hovering robots don't need one)
    const chair = new THREE.Group(); chair.position.set(0, 0, -0.95); g.add(chair);
    put(chair, U.box, M.leather, 0, 0.5, 0, 0.7, 0.1, 0.62); put(chair, U.box, M.leather, 0, 1.05, -0.32, 0.66, 1.0, 0.1);
    put(chair, U.box, accent(h), 0, 1.05, -0.38, 0.05, 0.9, 0.01, 0, false); put(chair, U.cyl, M.rib, 0, 0.25, 0, 0.08, 0.5, 0.08); put(chair, U.cyl, M.dark, 0, 0.03, 0, 0.6, 0.05, 0.6);
    return { screen: scr, chair };
  }
  const holoMats = {};
  function holoMat(s) {
    return holoMats[s] || (holoMats[s] = flat(new THREE.MeshBasicMaterial({ map: holoTex(s), transparent: true, opacity: 0.5, side: THREE.DoubleSide, toneMapped: false, depthWrite: false })));
  }

  // ---------------------------------------------------------------- furniture
  function pod(grp, x, z) { // hydroponic plant pod
    put(grp, U.cyl, M.rib, x, 0.12, z, 0.7, 0.24, 0.7); put(grp, U.cyl, M.rib, x, 1.62, z, 0.7, 0.1, 0.7);
    put(grp, U.cyl, M.pane, x, 0.9, z, 0.62, 1.4, 0.62, 0, false);
    for (let i = 0; i < 6; i++) put(grp, U.ico, M.leaf, x + Math.cos(i * 2.1) * 0.12, 0.45 + i * 0.14, z + Math.sin(i * 2.1) * 0.12, 0.3, 0.38, 0.3, i);
    const gl = put(grp, U.torus, led('#e879f9', 3), x, 1.56, z, 0.62, 0.62, 0.62, 0, false); gl.rotation.x = Math.PI / 2;
  }
  function sofa(grp, x, z, len, ry, h) {
    const g = new THREE.Group(); g.position.set(x, 0, z); g.rotation.y = ry; grp.add(g);
    put(g, U.box, M.dark, 0, 0.2, 0, len, 0.22, 0.95);
    put(g, U.box, M.leather, 0, 0.42, 0.05, len - 0.4, 0.2, 0.82);
    put(g, U.box, M.leather, 0, 0.72, -0.36, len, 0.6, 0.24);
    [-1, 1].forEach(sx => put(g, U.box, M.leather, sx * (len / 2 - 0.12), 0.5, 0, 0.24, 0.42, 0.95));
    put(g, U.box, accent(h), 0, 0.1, 0.48, len, 0.02, 0.01, 0, false);
  }
  function rack(grp, x, z, ry) {
    const g = new THREE.Group(); g.position.set(x, 0, z); g.rotation.y = ry; grp.add(g);
    put(g, U.box, M.dark, 0, 1.1, 0, 0.9, 2.2, 0.8);
    put(g, U.plane, flat(once('rackFace', () => new THREE.MeshStandardMaterial({ color: '#000', emissive: '#fff', emissiveMap: rackTex, emissiveIntensity: 2, toneMapped: false }))), 0, 1.1, 0.41, 0.8, 2.0, 1, 0, false);
  }
  function viewport(grp, x, z, ry, w = 3.0, hh = 1.9) {
    const g = new THREE.Group(); g.position.set(x, 1.95, z); g.rotation.y = ry; grp.add(g);
    put(g, U.plane, M.space, 0, 0, 0.01, w, hh, 1, 0, false);
    put(g, U.box, M.rib, 0, hh / 2 + 0.08, 0.08, w + 0.4, 0.16, 0.2); put(g, U.box, M.rib, 0, -hh / 2 - 0.08, 0.1, w + 0.4, 0.16, 0.26);
    [-1, 1].forEach(sx => put(g, U.box, M.rib, sx * (w / 2 + 0.12), 0, 0.08, 0.24, hh + 0.3, 0.2));
    put(g, U.box, led('#5ad8ff', 2.4), 0, -hh / 2 + 0.01, 0.2, w, 0.02, 0.02, 0, false);
    put(g, U.box, M.rib, 0, 0, 0.05, 0.06, hh, 0.06);
  }
  function wallScreen(grp, x, z, ry, k, w = 1.3, hh = 0.8, y = 1.95) {
    const g = new THREE.Group(); g.position.set(x, y, z); g.rotation.y = ry; grp.add(g);
    put(g, U.box, M.dark, 0, 0, 0.03, w + 0.1, hh + 0.1, 0.06); put(g, U.plane, screenMat(k), 0, 0, 0.065, w, hh, 1, 0, false);
  }


  // ---------------------------------------------------------------- the ship's exterior
  // The deck is a cutaway section of a big ship: hull and keel under it, a superstructure behind it,
  // a spine with spherical modules and an engine block to the left, a domed bridge to the right.
  // Hull blocks get world-scaled panel UVs, lit window dots (emissive map) and instanced greebles.
  function panelTex() {
    return tex(512, 512, (g, w, h) => {
      g.fillStyle = '#8b929c'; g.fillRect(0, 0, w, h);
      for (let i = 0; i < 60; i++) { const pw = 40 + rnd() * 160, ph = 30 + rnd() * 120, x = rnd() * w, y = rnd() * h; g.fillStyle = `hsl(215, 6%, ${44 + rnd() * 18}%)`; g.fillRect(x, y, pw, ph); g.strokeStyle = 'rgba(20,24,30,.55)'; g.lineWidth = 2; g.strokeRect(x, y, pw, ph); }
      for (let i = 0; i < 4000; i++) { g.fillStyle = `rgba(0,0,0,${rnd() * 0.08})`; g.fillRect(rnd() * w, rnd() * h, 2, 2); }
      g.strokeStyle = 'rgba(15,18,24,.7)'; g.lineWidth = 3; for (let x = 0; x <= w; x += 128) { g.beginPath(); g.moveTo(x, 0); g.lineTo(x, h); g.stroke(); } for (let y = 0; y <= h; y += 128) { g.beginPath(); g.moveTo(0, y); g.lineTo(w, y); g.stroke(); }
      g.fillStyle = 'rgba(30,34,40,.85)'; for (let i = 0; i < 40; i++) g.fillRect(rnd() * w, rnd() * h, 14, 7); // dark window frames
    }, 1, 1);
  }
  function lightsTex() { // lit windows: a few strips and dots
    return tex(512, 512, (g, w, h) => {
      g.fillStyle = '#000'; g.fillRect(0, 0, w, h);
      for (let i = 0; i < 26; i++) { const x = rnd() * w, y = rnd() * h, n = 2 + Math.floor(rnd() * 6); for (let k = 0; k < n; k++) { g.fillStyle = rnd() < 0.8 ? '#ffd27a' : '#9fe3ff'; g.fillRect(x + k * 12, y, 7, 5); } }
      for (let i = 0; i < 40; i++) { g.fillStyle = rnd() < 0.85 ? '#ffd27a' : '#9fe3ff'; g.fillRect(rnd() * w, rnd() * h, 4, 4); }
    }, 1, 1);
  }
  const HULL = once('hullExt', () => {
    const map = panelTex(), em = lightsTex();
    [map, em].forEach(t => { t.wrapS = t.wrapT = THREE.RepeatWrapping; });
    return new THREE.MeshStandardMaterial({ color: '#a7adb6', map, emissiveMap: em, emissive: '#ffffff', emissiveIntensity: 1.8, roughness: 0.5, metalness: 0.6 });
  });
  const HULL_DARK = once('hullDark', () => new THREE.MeshStandardMaterial({ color: '#4a5059', roughness: 0.45, metalness: 0.7 }));
  function hullGeo(w, h, d, scale = 5) { // a box whose texture repeats per world unit, so panels keep their size
    const geo = new THREE.BoxGeometry(w, h, d), uv = geo.attributes.uv;
    const dims = [[d, h], [d, h], [w, d], [w, d], [w, h], [w, h]];
    for (let f = 0; f < 6; f++) for (let v = 0; v < 4; v++) { const i = f * 4 + v; uv.setXY(i, uv.getX(i) * dims[f][0] / scale, uv.getY(i) * dims[f][1] / scale); }
    return geo;
  }
  const greebles = [];
  function block(grp, x, y, z, w, h, d, mat = SH || HULL, density = 1) {
    const m = new THREE.Mesh(hullGeo(w, h, d), mat); m.position.set(x, y, z); m.castShadow = m.receiveShadow = true; grp.add(m);
    // greebles on the top and on the faces we can see (+x, +z)
    const n = Math.round(w * d * 0.12 * density);
    for (let i = 0; i < n; i++) greebles.push([x + (rnd() - 0.5) * (w - 0.4), y + h / 2, z + (rnd() - 0.5) * (d - 0.4), 0.2 + rnd() * 0.9, 0.08 + rnd() * 0.35, 0.2 + rnd() * 0.9]);
    const nf = Math.round(w * h * 0.05 * density);
    for (let i = 0; i < nf; i++) greebles.push([x + (rnd() - 0.5) * (w - 0.4), y + (rnd() - 0.5) * (h - 0.3), z + d / 2, 0.2 + rnd() * 0.7, 0.1 + rnd() * 0.4, 0.06 + rnd() * 0.2]);
    return m;
  }
  function glowSprite(color) {
    return once('glow' + color, () => new THREE.SpriteMaterial({ map: tex(128, 128, (g) => { const r = g.createRadialGradient(64, 64, 0, 64, 64, 64); r.addColorStop(0, '#ffffff'); r.addColorStop(0.25, color); r.addColorStop(1, 'rgba(0,0,0,0)'); g.fillStyle = r; g.fillRect(0, 0, 128, 128); }), blending: THREE.AdditiveBlending, depthWrite: false, transparent: true, toneMapped: false }));
  }
  function navLight(grp, x, y, z, color) {
    put(grp, U.sph, led(color, 4), x, y, z, 0.25, 0.25, 0.25, 0, false);
    const s = new THREE.Sprite(glowSprite(color)); s.position.set(x, y, z); s.scale.setScalar(1.6); grp.add(s);
  }
  // ---------------------------------------------------------------- ship styles (one per hivey session)
  // hull tint, darker trim, light colour and aft design; style 0 is the classic frigate
  const STYLES = [
    { hull: null, dark: null, acc: '#36d6ff', kind: 0 },
    { hull: '#e0a066', dark: '#5a3a22', acc: '#ffb347', kind: 1 },
    { hull: '#7fb2e6', dark: '#1f3550', acc: '#5dff9a', kind: 2 },
    { hull: '#a98ee0', dark: '#2e2448', acc: '#ff5ad8', kind: 0 },
    { hull: '#86c99a', dark: '#1f3a2a', acc: '#ffd23a', kind: 1 },
    { hull: '#e08a8a', dark: '#4a2424', acc: '#7fe8ff', kind: 2 },
  ];
  let SH = null, SHD = null, ACC = '#36d6ff', KIND = 0, NAME = '', RUN = true, PLATE = null;
  // engines: a running session's ship thrusts (glow and a flickering plume), a stopped one's are cold
  const engines = [];
  const nozzle = () => RUN ? led(ACC, 5) : once('coldNozzle', () => new THREE.MeshStandardMaterial({ color: '#141922', roughness: 0.55, metalness: 0.85 }));
  function flame(grp, x, y, z, size) {
    if (!RUN) return;
    const sp = new THREE.Sprite(glowSprite(ACC)); sp.position.set(x, y, z); sp.scale.setScalar(size); grp.add(sp);
    const mat = once('plume:' + ACC, () => flat(new THREE.MeshBasicMaterial({ color: ACC, transparent: true, opacity: 0.32, blending: THREE.AdditiveBlending, depthWrite: false, toneMapped: false, side: THREE.DoubleSide })));
    const pl = new THREE.Mesh(U.cone, mat); pl.rotation.z = Math.PI / 2; pl.position.set(x - size * 0.3, y, z); pl.scale.set(size * 0.22, size * 0.8, size * 0.22); grp.add(pl);
    engines.push({ sp, pl, s: size, x, ph: rnd() * 6 });
  }
  function thrust(s) {
    for (const e of engines) {
      if (!e.sp.parent?.parent) continue;  // a ship from an older build
      const k = 1 + 0.1 * Math.sin(s * 23 + e.ph) + 0.05 * Math.sin(s * 61 + e.ph * 2);
      e.sp.scale.setScalar(e.s * k);
      e.pl.scale.y = e.s * 0.8 * (1 + 0.18 * Math.sin(s * 31 + e.ph));
      e.pl.position.x = e.x - e.pl.scale.y * 0.38;
    }
  }
  function useStyle(v, name, running) {
    RUN = running !== false;
    const st = STYLES[((v | 0) % STYLES.length + STYLES.length) % STYLES.length];
    SH = st.hull ? once('hull:' + st.hull, () => { const m = HULL.clone(); m.color.set(st.hull); return m; }) : HULL;
    SHD = st.dark ? once('dark:' + st.dark, () => { const m = HULL_DARK.clone(); m.color.set(st.dark); return m; }) : HULL_DARK;
    ACC = st.acc; KIND = st.kind; NAME = name ? String(name).toUpperCase() : '';
    for (let i = engines.length - 1; i >= 0; i--) if (!engines[i].sp.parent?.parent) engines.splice(i, 1);
  }
  // the ship's name painted on its hull, in its light colour
  function nameplate(grp, text, x, y, z, w, h) {
    if (!text) return;
    const map = tex(1024, 160, (g, cw, ch) => {
      g.clearRect(0, 0, cw, ch);
      g.font = '800 104px "Avenir Next", "Segoe UI", Arial, sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle';
      let size = 104; while (g.measureText(text).width > cw - 60 && size > 40) { size -= 6; g.font = `800 ${size}px "Avenir Next", "Segoe UI", Arial, sans-serif`; }
      g.lineWidth = 10; g.strokeStyle = 'rgba(8,12,20,.85)'; g.strokeText(text, cw / 2, ch / 2 + 4);
      g.fillStyle = '#f4f7fb'; g.fillText(text, cw / 2, ch / 2 + 4);
      g.fillStyle = ACC; g.fillRect(30, ch - 14, cw - 60, 6);
    }, 1, 1);
    const mat = flat(new THREE.MeshBasicMaterial({ map, transparent: true, toneMapped: false, depthWrite: false }));
    const m = new THREE.Mesh(U.plane, mat); m.position.set(x, y, z); m.scale.set(Math.min(w, h * 1024 / 160), Math.min(h, w * 160 / 1024), 1); m.rotation.x = -0.55; grp.add(m);
  }
  function thruster(grp, x, y, z, r) {
    const n = put(grp, U.cyl, SHD, x, y, z, r * 2.1, r * 1.8, r * 2.1); n.rotation.z = Math.PI / 2;
    const c = put(grp, U.circ, nozzle(), x - r * 0.92, y, z, r * 1.8, r * 1.8, 1, 0, false); c.rotation.y = -Math.PI / 2;
    flame(grp, x - r * 1.8, y, z, r * 5);
  }
  // carrier: a broad aft hull with two long engine nacelles on outriggers
  function carrierAft(grp, x0, z0, z1, cz, sy, sz) {
    const L = 22, hz = (z1 - z0) / 2 + 1.5;
    block(grp, x0 - 1 - L / 2, sy - 0.4, cz, L, 3.2, (z1 - z0) * 0.8, SH, 0.6);
    block(grp, x0 - 6 - L / 2, sy + 2.4, cz - 1, L * 0.6, 2.2, (z1 - z0) * 0.4, SHD, 0.6);
    [-1, 1].forEach(sd => {
      const nz = cz + sd * hz, len = 30, nx = x0 - 4 - len / 2;
      const body = new THREE.Mesh(new THREE.CylinderGeometry(2.1, 2.4, len, 28), SH); body.rotation.z = Math.PI / 2; body.position.set(nx, sy, nz); body.castShadow = body.receiveShadow = true; grp.add(body);
      for (let i = 0; i < 5; i++) { const b = put(grp, U.torus, SHD, nx - len / 2 + 4 + i * 5.5, sy, nz, 4.9, 4.9, 4.9); b.rotation.y = Math.PI / 2; }
      put(grp, U.box, SHD, x0 - 10, sy, cz + sd * (hz - 2.4), 5, 0.8, 3.4); // pylon
      const cap = put(grp, U.circ, led(ACC, 3), nx + len / 2 + 0.02, sy, nz, 3.6, 3.6, 1, 0, false); cap.rotation.y = Math.PI / 2;
      thruster(grp, nx - len / 2 - 0.6, sy, nz, 2.2);
      for (let i = 0; i < 7; i++) put(grp, U.box, led('#ffd27a', 2.2), nx - 9 + i * 2.6, sy + 1.2, nz + sd * 1.9, 0.5, 0.18, 0.05, 0, false);
    });
    navLight(grp, x0 - 34, sy + 2.6, cz + hz, '#3bff7a'); navLight(grp, x0 - 34, sy + 2.6, cz - hz, '#ff3b3b');
  }
  // explorer: a long thin spine through a big habitat ring, cargo pods and one large engine bell
  function explorerAft(grp, x0, cz, sy, sz) {
    const len = 40, mx = x0 - 1.5 - len / 2;
    const spine = new THREE.Mesh(new THREE.CylinderGeometry(1.1, 1.1, len, 20), SHD); spine.rotation.z = Math.PI / 2; spine.position.set(mx, sy, sz); spine.castShadow = true; grp.add(spine);
    const rx = x0 - 13;
    const ring = new THREE.Mesh(new THREE.TorusGeometry(7.5, 0.9, 16, 64), SH); ring.rotation.y = Math.PI / 2; ring.position.set(rx, sy, sz); ring.castShadow = ring.receiveShadow = true; grp.add(ring);
    for (let i = 0; i < 4; i++) { const sp = put(grp, U.box, SHD, rx, sy, sz, 0.35, 15, 0.35); sp.rotation.x = i * Math.PI / 4; }
    for (let i = 0; i < 24; i++) { const t = i / 24 * Math.PI * 2; put(grp, U.box, led(i % 3 ? '#ffd27a' : ACC, 2.2), rx + 0.92, sy + Math.sin(t) * 7.5, sz + Math.cos(t) * 7.5, 0.05, 0.3, 0.3, 0, false); }
    [[-24, 2.4], [-24, -2.4], [-29, 2.4], [-29, -2.4]].forEach(([dx, dz]) => { const c = put(grp, U.cap, SH, x0 + dx, sy, sz + dz, 1.6, 2.2, 1.6); c.rotation.z = Math.PI / 2; });
    const ex = x0 - 1.5 - len;
    block(grp, ex + 2, sy, sz, 4, 4.4, 4.4, SH, 0.5);
    const bell = new THREE.Mesh(new THREE.ConeGeometry(4.2, 5.5, 32, 1, true), SHD); bell.rotation.z = -Math.PI / 2; bell.position.set(ex - 2.6, sy, sz); bell.material.side = THREE.DoubleSide; grp.add(bell);
    const c = put(grp, U.circ, nozzle(), ex - 0.4, sy, sz, 4.6, 4.6, 1, 0, false); c.rotation.y = -Math.PI / 2;
    flame(grp, ex - 4.5, sy, sz, 11);
    navLight(grp, rx, sy + 8.6, sz, '#ffffff');
  }

  function exterior(grp, x0, x1, z0, z1) {
    greebles.length = 0;
    const fw = x1 - x0, fd = z1 - z0, cx = (x0 + x1) / 2, cz = (z0 + z1) / 2;
    // hull under the deck: plating, a stepped keel, a low rim at the open edges
    block(grp, cx, -1.35, cz, fw + 1.6, 2.4, fd + 1.6, SH, 0.3);
    block(grp, cx - 2, -3.3, cz - 1, fw * 0.82, 1.6, fd * 0.7, SH, 0.4);
    block(grp, cx - 4, -4.6, cz - 2, fw * 0.55, 1.0, fd * 0.4, SHD, 0.5);
    put(grp, U.box, SHD, cx, 0.22, z1 + 0.5, fw + 1.6, 0.5, 0.6); put(grp, U.box, SHD, x1 + 0.5, 0.22, cz, 0.6, 0.5, fd + 1.6);
    put(grp, U.box, led(ACC, 2.2), cx, 0.48, z1 + 0.82, fw + 1.6, 0.04, 0.04, 0, false); put(grp, U.box, led(ACC, 2.2), x1 + 0.82, 0.48, cz, 0.04, 0.04, fd + 1.6, 0, false);
    // superstructure behind the back wall: stepped decks, a command tower, a dish, antennas, radiator wings
    block(grp, cx, 1.1, z0 - 3.6, fw + 1.6, 5.0, 6.6);
    let x = x0 + 1;
    while (x < x1 - 2) { const w = 4 + rnd() * 7, hh = 1 + rnd() * 2.2, d = 2.4 + rnd() * 2.6; block(grp, Math.min(x + w / 2, x1 - w / 2), 3.6 + hh / 2, z0 - 1.8 - d / 2 - rnd() * 1.5, w, hh, d); x += w + 0.6 + rnd() * 2; }
    const tx = cx + fw * 0.12, tz = z0 - 4.5;
    block(grp, tx, 7.2, tz, 4.2, 4.2, 3.6); block(grp, tx, 9.9, tz, 2.6, 1.4, 2.4, SHD);
    put(grp, U.sph, SH, tx, 10.6, tz, 2.4, 1.4, 2.4);
    put(grp, U.box, led('#9fe3ff', 2.5), tx, 8.4, tz + 1.82, 3.6, 0.35, 0.04, 0, false); // tower bridge windows
    [[-0.8, 2.6], [0.6, 3.6], [1.1, 1.8]].forEach(([dx, hgt]) => { put(grp, U.cyl, SHD, tx + dx, 11.2 + hgt / 2, tz, 0.08, hgt, 0.08); navLight(grp, tx + dx, 11.2 + hgt, tz, '#ffffff'); });
    if (!place(grp, 'dishL', 3.2, tx - fw * 0.3, 3.6, z0 - 3.4, 0.6)) { const dish = put(grp, U.sph, SHD, tx - fw * 0.3, 5.6, z0 - 3.4, 3.2, 1.0, 3.2); dish.rotation.x = -0.5; }
    place(grp, 'dish', 3, cx - fw * 0.42, 3.6, z0 - 1.6, -0.4); place(grp, 'wireless', 3.5, cx + fw * 0.38, 3.6, z0 - 2.2); place(grp, 'wireless', 3.5, cx - fw * 0.05, 3.6, z0 - 5.5, 1);
    const radMat = once('radiator', () => new THREE.MeshStandardMaterial({ map: tex(256, 128, (g, w, h) => { g.fillStyle = '#16233f'; g.fillRect(0, 0, w, h); g.strokeStyle = '#3a5ea8'; g.lineWidth = 2; for (let i = 0; i < w; i += 16) { g.beginPath(); g.moveTo(i, 0); g.lineTo(i, h); g.stroke(); } for (let j = 0; j < h; j += 16) { g.beginPath(); g.moveTo(0, j); g.lineTo(w, j); g.stroke(); } }), roughness: 0.3, metalness: 0.8 }));
    if (KIND === 0) [-1, 1].forEach(sd => { const r = put(grp, U.box, radMat, cx + sd * fw * 0.28, 4.2, z0 - 10.5, fw * 0.3, 0.12, 6.5); r.rotation.x = 0.12; put(grp, U.box, SHD, cx + sd * fw * 0.28, 4.0, z0 - 7.2, 0.5, 0.5, 1.6); });
    if (KIND !== 0) [-1, 1].forEach(sd => { const f = put(grp, U.box, SHD, cx + sd * fw * 0.3, 5.2, z0 - 6.5, 0.4, 3.4, 5.5); f.rotation.x = -0.35; put(grp, U.box, led(ACC, 2.4), cx + sd * fw * 0.3, 6.85, z0 - 7.1, 0.42, 0.06, 4.2, 0, false).rotation.x = -0.35; });
    nameplate(grp, NAME, cx - fw * 0.12, -1.2, z1 + 1.15, Math.min(fw * 0.75, 19), 3.0);
    PLATE = { x: cx - fw * 0.12, y: -1.2, z: z1 + 1.15, w: Math.min(fw * 0.75, 19, 3.0 * 1024 / 160) };  // the page puts the name's click area here
    // aft section: one of three designs, so every hivey session flies its own kind of ship
    const sy = 0.2, sz = cz - 2.5;
    if (KIND === 1) carrierAft(grp, x0, z0, z1, cz, sy, sz);
    else if (KIND === 2) explorerAft(grp, x0, cz, sy, sz);
    else {
      const len = 30;
      const spine = put(grp, U.box, SH, x0 - 1.5 - len / 2, sy, sz, len, 2.8, 3.2); spine.geometry = hullGeo(len, 2.8, 3.2); spine.scale.set(1, 1, 1);
      [x0 - 10, x0 - 18.5].forEach(px => {
        const sph = new THREE.Mesh(new THREE.SphereGeometry(4, 40, 28), SH); sph.position.set(px, sy, sz); sph.castShadow = sph.receiveShadow = true;
        sph.material = SH; grp.add(sph);
        const band = put(grp, U.torus, SHD, px, sy, sz, 8.2, 8.2, 8.2, 0, true); band.rotation.y = Math.PI / 2;
        for (let i = 0; i < 6; i++) put(grp, U.box, led('#ffd27a', 2.2), px - 1.6 + i * 0.6, sy + 2.6 - (i % 2) * 0.3, sz + 3.05, 0.25, 0.14, 0.05, 0, false);
      });
      for (let i = 0; i < 8; i++) put(grp, U.box, SHD, x0 - 4 - i * 3.4, sy + 1.6, sz, 0.3, 0.4, 3.6); // spine ribs
      const ex = x0 - 1.5 - len - 4.5;
      block(grp, ex, sy + 0.6, sz, 9, 8, 11);
      block(grp, ex + 1, sy + 5.6, sz - 1, 5, 2, 7, SHD);
      [[-2.4, -2.4], [-2.4, 2.4], [2.0, -2.4], [2.0, 2.4]].forEach(([dy, dz]) => {
        const n = put(grp, U.cyl, SHD, ex - 5.4, sy + 0.6 + dy, sz + dz, 2.2, 2.0, 2.2); n.rotation.z = Math.PI / 2;
        const c = put(grp, U.circ, nozzle(), ex - 6.42, sy + 0.6 + dy, sz + dz, 1.9, 1.9, 1, 0, false); c.rotation.y = -Math.PI / 2;
        flame(grp, ex - 7.4, sy + 0.6 + dy, sz + dz, 5.5);
      });
      navLight(grp, ex - 1, sy + 6.8, sz + 5.6, '#ff3b3b');
    }
    // all greebles in two instanced draws
    [SHD, SH].forEach((mat, k) => {
      const list = greebles.filter((_, i) => i % 2 === k);
      const inst = new THREE.InstancedMesh(U.box, mat, list.length), m4 = new THREE.Matrix4(), q = new THREE.Quaternion();
      list.forEach(([gx, gy, gz, w, h, d], i) => inst.setMatrixAt(i, m4.compose(new THREE.Vector3(gx, gy, gz), q, new THREE.Vector3(w, h, d))));
      inst.castShadow = inst.receiveShadow = true; inst.computeBoundingBox?.(); inst.computeBoundingSphere(); grp.add(inst);
    });
  }


  // ---------------------------------------------------------------- the captain's bridge (the CEO's room)
  // A separate cutaway room in the nose: its own hull, a main viewscreen, gold trim, side consoles.
  // Returns where to put the CEO's desks and the room label.
  function bridge(grp, e) {
    const BW = 10, BD = Math.min(Math.max((e.z1 - e.z0) * 0.7, 9), 12), cz = (e.z0 + e.z1) / 2 - 0.5;
    const bx0 = e.x1 + 1.6, bx1 = bx0 + BW, bz0 = cz - BD / 2, bz1 = cz + BD / 2, bxc = (bx0 + bx1) / 2;
    const gold = led('#ffc94a', 2.2);
    block(grp, bxc, -1.35, cz, BW + 1.2, 2.4, BD + 1.2, SH, 0.3);
    block(grp, bxc + 1, -3.1, cz, BW * 0.8, 1.4, BD * 0.6, SHD, 0.4);
    const fl = put(grp, U.plane, M.deck, bxc, 0.004, cz, BW, BD, 1, 0, false); fl.rotation.x = -Math.PI / 2; fl.receiveShadow = true;
    const inlay = put(grp, U.ring, gold, bxc, 0.012, cz + 0.4, 3.4, 3.4, 1, 0, false); inlay.rotation.x = -Math.PI / 2;
    // connecting gangway from the main deck
    const gw = put(grp, U.plane, M.deck, (e.x1 + bx0) / 2 + 0.3, 0.003, cz, bx0 - e.x1 + 0.6, 2.4, 1, 0, false); gw.rotation.x = -Math.PI / 2;
    put(grp, U.box, SH, (e.x1 + bx0) / 2 + 0.3, -0.7, cz, bx0 - e.x1 + 0.6, 1.4, 2.6);
    // back wall with the main viewscreen, left wall with a doorway, low rims in front and at the nose
    put(grp, U.box, M.hull, bxc, 1.7, bz0 - 0.15, BW + 0.3, 3.4, 0.3);
    viewport(grp, bxc, bz0 + 0.02, 0, 6.4, 2.4);
    put(grp, U.box, gold, bxc, 3.25, bz0 + 0.03, BW, 0.06, 0.04, 0, false);
    [[bz0, cz - 1.3], [cz + 1.3, bz1]].forEach(([a, b]) => put(grp, U.box, M.hull, bx0 - 0.15, 1.7, (a + b) / 2, 0.3, 3.4, b - a));
    put(grp, U.box, SHD, bxc, 0.25, bz1 + 0.3, BW + 0.6, 0.5, 0.6); put(grp, U.box, gold, bxc, 0.52, bz1 + 0.58, BW + 0.6, 0.04, 0.04, 0, false);
    put(grp, U.box, SHD, bx1 + 0.3, 0.25, cz, 0.6, 0.5, BD + 0.6); put(grp, U.box, gold, bx1 + 0.58, 0.52, cz, 0.04, 0.04, BD + 0.6, 0, false);
    const dome = new THREE.Mesh(new THREE.SphereGeometry(2.4, 40, 20, 0, Math.PI * 2, 0, Math.PI / 2), SH); dome.position.set(bx1 - 2, 3.4, bz0 - 1.2); dome.castShadow = true; grp.add(dome);
    block(grp, bx1 - 2, 1.7, bz0 - 1.3, 4.6, 3.4, 2.2);
    // the nose beyond the bridge
    const nose = new THREE.Mesh(new THREE.CylinderGeometry(1.4, 3.2, 5.2, 6), SH); nose.rotation.z = -Math.PI / 2; nose.position.set(bx1 + 3.2, -0.4, cz); nose.castShadow = true; grp.add(nose);
    navLight(grp, bx1 + 5.9, -0.4, cz, '#3bff7a');
    // side consoles and the captain's lounge chairs
    place(grp, 'consoleWide', 2.4, bx0 + 0.9, 0, bz0 + 1.0, 0.6); place(grp, 'consoleWide', 2.4, bx1 - 0.9, 0, bz0 + 1.0, -0.6);
    place(grp, 'lounger', 2.4, bx1 - 1.2, 0, bz1 - 1.4, -2.2);
    pod(grp, bx0 + 0.7, bz1 - 0.8);
    return {
      slots: [{ x: bxc, z: cz + 0.4, captain: true }, { x: bxc - 3.4, z: cz + 2.2 }, { x: bxc + 3.4, z: cz + 2.2 }, { x: bxc, z: cz + 4.0 }],
      label: new THREE.Vector3(bx0 + 0.3, 0.9, bz1 + 0.2),
      bounds: { maxX: bx1 + 6.5 },
    };
  }

  // ---------------------------------------------------------------- the deck
  let holoPlanet = null;
  const planets = [];  // one holotable planet per ship
  function decorate(grp, W, D, style = {}) {
    useStyle(style.v || 0, style.name, style.running);
    seedN = 11;
    const m = 2.8, L = 10, H = 3.4;
    const x0 = -W / 2 - m, x1 = W / 2 + m + L, z0 = -D / 2 - m, z1 = D / 2 + 1.4;
    const fw = x1 - x0, fd = z1 - z0;
    M.deck.map.repeat.set(fw / 3, fd / 3); M.deck.map.wrapS = M.deck.map.wrapT = THREE.RepeatWrapping;
    const fl = put(grp, U.plane, M.deck, (x0 + x1) / 2, 0, (z0 + z1) / 2, fw, fd, 1, 0, false); fl.rotation.x = -Math.PI / 2; fl.receiveShadow = true;
    exterior(grp, x0, x1, z0, z1);
    // hull walls with ribs, light strips and viewports
    M.hull.map.wrapS = M.hull.map.wrapT = THREE.RepeatWrapping;
    put(grp, U.box, M.hull, (x0 + x1) / 2, H / 2, z0 - 0.15, fw + 0.3, H, 0.3);
    put(grp, U.box, M.hull, x0 - 0.15, H / 2, (z0 + z1) / 2, 0.3, H, fd);
    put(grp, U.box, led('#36d6ff', 2.5), (x0 + x1) / 2, H - 0.18, z0 + 0.02, fw, 0.05, 0.04, 0, false);
    put(grp, U.box, led('#36d6ff', 2.5), x0 + 0.02, H - 0.18, (z0 + z1) / 2, 0.04, 0.05, fd, 0, false);
    put(grp, U.box, led('#36d6ff', 1.2), (x0 + x1) / 2, 0.08, z0 + 0.02, fw, 0.03, 0.03, 0, false);
    for (let x = x0 + 0.4; x < x1; x += 2.6) put(grp, U.box, M.rib, x, H / 2, z0 + 0.05, 0.22, H, 0.2);
    for (let z = z0 + 0.4; z < z1; z += 2.6) put(grp, U.box, M.rib, x0 + 0.05, H / 2, z, 0.2, H, 0.22);
    let k = 0;
    for (let x = x0 + 3.2; x < x1 - L - 1.6; x += 6.5) wallScreen(grp, x, z0 + 0.06, 0, k++ % 4, 1.5, 0.9, 1.9);
    for (let z = z0 + 3.4; z < z1 - 1.6; z += 6.5) wallScreen(grp, x0 + 0.06, z, Math.PI / 2, k++ % 4, 1.5, 0.9, 1.9);
    [0.55, 2.75].forEach(y => { // conduits along the hull walls
      put(grp, U.cyl, M.rib, (x0 + x1 - L) / 2, y, z0 + 0.22, 0.14, x1 - L - x0, 0.14, 0, true).rotation.z = Math.PI / 2;
      put(grp, U.cyl, M.rib, x0 + 0.22, y, (z0 + z1) / 2, 0.14, fd, 0.14, 0, true).rotation.x = Math.PI / 2;
    });
    put(grp, U.cyl, accent(28), (x0 + x1 - L) / 2, 2.95, z0 + 0.22, 0.07, x1 - L - x0, 0.07, 0, false).rotation.z = Math.PI / 2;
    // a bulkhead door on the side wall
    const dz = (z0 + z1) / 2 + 3.2;
    put(grp, U.box, M.dark, x0 + 0.08, 1.2, dz, 0.1, 2.4, 1.6); put(grp, U.box, led('#36d6ff', 2), x0 + 0.14, 1.2, dz, 0.02, 2.5, 0.05, 0, false);
    put(grp, U.box, M.hazard, x0 + 0.14, 2.5, dz, 0.02, 0.12, 1.7, 0, false);
    // observation lounge: a big viewport, sofas around a holotable with a turning planet
    const lx = x1 - L / 2, lz = Math.min(z0 + 6.4, (z0 + z1) / 2 + 0.6);
    viewport(grp, lx, z0 + 0.06, 0, 6.5, 2.4);
    const disc = put(grp, U.circ, flat(once('loungeFloor', () => new THREE.MeshStandardMaterial({ color: '#0e1b2a', roughness: 0.3, metalness: 0.6 }))), lx, 0.01, lz, 5.6, 5.6, 1, 0, false); disc.rotation.x = -Math.PI / 2;
    const rim = put(grp, U.ring, led('#36d6ff', 1.6), lx, 0.015, lz, 5.7, 5.7, 1, 0, false); rim.rotation.x = -Math.PI / 2;
    sofa(grp, lx, lz - 1.7, 3.4, 0, 200); sofa(grp, lx + 2.3, lz + 0.1, 2.4, -Math.PI / 2, 200); sofa(grp, lx - 2.3, lz + 0.1, 2.4, Math.PI / 2, 200);
    put(grp, U.cyl, M.rib, lx, 0.25, lz, 1.2, 0.5, 1.2); put(grp, U.cyl, led('#36d6ff', 2.8), lx, 0.51, lz, 1.05, 0.03, 1.05, 0, false);
    holoPlanet = new THREE.Group(); holoPlanet.position.set(lx, 1.25, lz); grp.add(holoPlanet); planets.push(holoPlanet);
    holoPlanet.add(mesh(U.wire, flat(once('holoWire', () => new THREE.MeshBasicMaterial({ color: '#5ad8ff', wireframe: true, transparent: true, opacity: 0.55, toneMapped: false }))), 0, 0, 0, 0.9, 0.9, 0.9, false));
    holoPlanet.add(mesh(U.sph, flat(once('holoCore', () => new THREE.MeshBasicMaterial({ color: '#1e90ff', transparent: true, opacity: 0.25, toneMapped: false }))), 0, 0, 0, 0.8, 0.8, 0.8, false));
    const hr = mesh(U.torus, led('#ffcf3a', 2), 0, 0, 0, 1.4, 1.4, 1.4, false); hr.rotation.x = 1.2; holoPlanet.add(hr);
    // galley on the back wall: counter, replicator, dispenser, cups; hazard stripe in front
    const cx = x1 - L + 0.5;
    put(grp, U.box, M.slab, cx + 1.9, 0.46, z0 + 0.45, 3.8, 0.92, 0.75); put(grp, U.box, M.white, cx + 1.9, 0.94, z0 + 0.45, 3.9, 0.05, 0.8);
    put(grp, U.box, accent(190), cx + 1.9, 0.9, z0 + 0.86, 3.8, 0.03, 0.01, 0, false);
    put(grp, U.box, M.dark, cx + 0.8, 1.35, z0 + 0.35, 0.8, 0.8, 0.5); put(grp, U.box, led('#ffb020', 2.5), cx + 0.8, 1.32, z0 + 0.61, 0.5, 0.25, 0.01, 0, false);
    put(grp, U.cyl, M.white, cx + 2.0, 1.25, z0 + 0.4, 0.4, 0.6, 0.4); put(grp, U.cyl, led('#5ad8ff', 1.6), cx + 2.0, 1.5, z0 + 0.4, 0.42, 0.04, 0.42, 0, false);
    for (let i = 0; i < 4; i++) put(grp, U.cyl, M.white, cx + 2.6 + i * 0.2, 1.03, z0 + 0.55, 0.12, 0.14, 0.12);
    wallScreen(grp, cx + 3.2, z0 + 0.06, 0, 1, 1.0, 0.6, 1.75);
    put(grp, U.plane, M.hazard, cx + 1.9, 0.013, z0 + 1.1, 3.8, 0.18, 1, 0, false).rotation.x = -Math.PI / 2;
    // server racks and plant pods along the right side, more where the deck is deep
    if (MODELS.generatorL) {
      for (let i = 0; i < (fd > 13 ? 3 : 1); i++) place(grp, 'generatorL', 3, x1 - 0.9, 0, Math.min(lz + 3.4, z1 - 1.2) + i * 1.4, -Math.PI / 2);
      place(grp, 'astroA', 2.2, lx - 1.2, 0, lz + 1.6, 2.6); place(grp, 'astroB', 2.2, lx + 1.0, 0, lz + 1.9, -2.4);
      place(grp, 'barrelsRail', 2.6, x1 - L + 0.9, 0, z1 - 1.2, 0.3); place(grp, 'crateTall', 2.2, x1 - L + 2.2, 0, z1 - 0.9, 0.1); place(grp, 'crateWide', 2.2, x1 - L + 3.3, 0, z1 - 1.0, -0.2);
      if (lz + 4.6 < z1 - 2.6) { // shuttle bay: two small craft on glowing pads
        [[lx - 1.6, 'speeder', 1.4], [lx + 1.9, 'racer', 1.3]].forEach(([px, k, sc]) => {
          const pz = z1 - 2.4;
          const pad = put(grp, U.circ, flat(once('pad', () => new THREE.MeshStandardMaterial({ color: '#141b26', roughness: 0.4, metalness: 0.6 }))), px, 0.02, pz, 3.0, 3.0, 1, 0, false); pad.rotation.x = -Math.PI / 2;
          const pr = put(grp, U.ring, led('#ffb020', 2), px, 0.025, pz, 3.0, 3.0, 1, 0, false); pr.rotation.x = -Math.PI / 2;
          place(grp, k, sc, px, 0.05, pz, 0.5);
        });
      }
    } else {
      rack(grp, x1 - 0.5, Math.min(lz + 3.6, z1 - 1.0), -Math.PI / 2);
      if (fd > 13) { rack(grp, x1 - 0.5, lz + 4.6, -Math.PI / 2); rack(grp, x1 - 0.5, lz + 5.6, -Math.PI / 2); }
    }
    place(grp, 'consoleWide', 2.4, x0 + 0.7, 0, (z0 + z1) / 2 - 1.5, Math.PI / 2); place(grp, 'console', 2.4, x0 + 0.7, 0, (z0 + z1) / 2 + 0.2, Math.PI / 2);
    pod(grp, x1 - 0.7, z0 + 0.7); pod(grp, x0 + 0.7, z0 + 0.7); pod(grp, x0 + 0.7, z1 - 0.8); pod(grp, x1 - L - 0.5, z1 - 0.8);
    // bridge console in the corner by the lounge
    put(grp, U.box, M.slab, x1 - L - 0.5, 0.5, z0 + 0.7, 0.9, 1.0, 0.7); put(grp, U.plane, screenMat(0), x1 - L - 0.5, 1.02, z0 + 0.75, 0.75, 0.5, 1, 0, false).rotation.x = -1.0;
    const lamp = new THREE.PointLight('#5ad8ff', 8, 7, 1.5); lamp.position.set(lx, 1.6, lz); grp.add(lamp);
    return { minX: x0 - 6, maxX: x1 + 5, minZ: z0 - 4, maxZ: z1 + 1.2, x0, x1, z0, z1, plate: PLATE };
  }

  // the space outside the windows: one of six scenes, rolled again on every page load
  const SCENES = [
    { name: 'violet', deep: ['#0a0f2a', '#010208'], clouds: ['rgba(130,60,220,.30)', 'rgba(20,170,210,.22)', 'rgba(230,70,140,.18)', 'rgba(60,90,255,.18)'], arms: ['170,190,255', '255,170,220'], body: 'giant', ring: '230,200,160', planet: ['#f2c894', '#b36a3a', '#2a120a'] },
    { name: 'emerald', deep: ['#04201c', '#010605'], clouds: ['rgba(20,200,140,.26)', 'rgba(40,140,220,.20)', 'rgba(160,240,120,.14)', 'rgba(10,90,120,.24)'], arms: ['150,255,210', '170,210,255'], body: 'ice', planet: ['#e8fbff', '#6fc2e8', '#0b2a44'] },
    { name: 'crimson', deep: ['#24070c', '#060102'], clouds: ['rgba(230,60,50,.28)', 'rgba(255,140,40,.20)', 'rgba(170,30,110,.22)', 'rgba(90,20,60,.30)'], arms: ['255,190,150', '255,120,140'], body: 'sun', sun: ['255,120,60', '255,220,170'], moon: true },
    { name: 'deep blue', deep: ['#061433', '#00030b'], clouds: ['rgba(40,100,255,.28)', 'rgba(20,200,255,.16)', 'rgba(120,80,255,.16)', 'rgba(10,40,120,.30)'], arms: ['180,210,255', '200,180,255'], body: 'earth', planet: ['#7fd0ff', '#1f5fa8', '#04122a'], moon: true },
    { name: 'golden', deep: ['#201604', '#060401'], clouds: ['rgba(240,170,40,.24)', 'rgba(255,220,120,.14)', 'rgba(200,90,30,.20)', 'rgba(120,70,20,.26)'], arms: ['255,230,170', '255,190,120'], body: 'binary', sun: ['255,200,90', '255,250,220'] },
    { name: 'singularity', deep: ['#07050a', '#000000'], clouds: ['rgba(90,60,140,.10)', 'rgba(255,150,80,.06)', 'rgba(40,50,120,.10)', 'rgba(120,60,90,.06)'], arms: ['200,190,255', '255,180,140'], body: 'hole' },
  ];
  // which scene: ?space=1..6 in the address, else the page's 🌌 setting (a number, or 'random' for a new one each load)
  const sceneNo = want => { const n = parseInt(want, 10); return n >= 1 && n <= SCENES.length ? n : 1 + Math.floor(Math.random() * SCENES.length); };
  const fromUrl = parseInt(new URLSearchParams(globalThis.location?.search || '').get('space'), 10);
  function galaxyTex(n) {
    const R = Math.random, sc = SCENES[n - 1];
    const aspect = Math.min(2.6, Math.max(1.2, (globalThis.innerWidth || 16) / Math.max(1, (globalThis.innerHeight || 9) - 100)));
    return tex(2048, Math.round(2048 / aspect), (g, w, h) => {
      const glow = (x, y, r, inner, outer = 'rgba(0,0,0,0)') => { const n = g.createRadialGradient(x, y, 0, x, y, r); n.addColorStop(0, inner); n.addColorStop(1, outer); g.fillStyle = n; g.fillRect(0, 0, w, h); };
      const bg = g.createRadialGradient(w * 0.5, h * 0.5, 0, w * 0.5, h * 0.5, w * 0.7); bg.addColorStop(0, sc.deep[0]); bg.addColorStop(1, sc.deep[1]);
      g.fillStyle = bg; g.fillRect(0, 0, w, h);
      sc.clouds.forEach(c => { for (let k = 0; k < 2; k++) glow(R() * w, R() * h, (0.18 + R() * 0.22) * w, c); });
      for (let i = 0; i < 7000 + R() * 5000; i++) { const b = R(); g.fillStyle = `rgba(${190 + b * 65},${200 + b * 55},255,${0.25 + b * 0.75})`; const sz = b > 0.985 ? 3 : b > 0.9 ? 1.8 : 1; g.fillRect(R() * w, R() * h, sz, sz); }
      // the galaxy (and the body opposite it) lands in a random corner
      const right = R() < 0.5, top = R() < 0.5;
      const gx = w * (right ? 0.72 + R() * 0.12 : 0.16 + R() * 0.12), gy = h * (top ? 0.18 + R() * 0.1 : 0.7 + R() * 0.1);
      g.save(); g.translate(gx, gy); g.scale(1, 0.35 + R() * 0.3); g.rotate(-0.8 + R() * 1.6);
      const size = 0.7 + R() * 0.6;
      const core = g.createRadialGradient(0, 0, 0, 0, 0, 260 * size); core.addColorStop(0, 'rgba(255,240,210,.95)'); core.addColorStop(0.15, `rgba(${sc.arms[1]},.5)`); core.addColorStop(1, 'rgba(0,0,0,0)');
      g.fillStyle = core; g.beginPath(); g.arc(0, 0, 260 * size, 0, 7); g.fill();
      for (let arm = 0; arm < 2; arm++) for (let i = 0; i < 1600; i++) {
        const t = i / 1600 * 3.2 * Math.PI, rr = (18 + t * 26) * size, a = t + arm * Math.PI + (R() - 0.5) * 0.5;
        g.fillStyle = R() < 0.7 ? `rgba(${sc.arms[0]},${0.15 + R() * 0.5})` : `rgba(${sc.arms[1]},${0.2 + R() * 0.4})`;
        g.fillRect(Math.cos(a) * rr + (R() - 0.5) * 14, Math.sin(a) * rr + (R() - 0.5) * 14, 1.6, 1.6);
      }
      g.restore();
      const px = w * (right ? 0.1 + R() * 0.12 : 0.78 + R() * 0.12), py = h * (top ? 0.72 + R() * 0.12 : 0.2 + R() * 0.12), pr = 90 + R() * 70;
      const ball = (x, y, r, c) => { const pg = g.createRadialGradient(x - r * 0.4, y - r * 0.4, r * 0.08, x, y, r); pg.addColorStop(0, c[0]); pg.addColorStop(0.6, c[1]); pg.addColorStop(1, c[2]); g.fillStyle = pg; g.beginPath(); g.arc(x, y, r, 0, 7); g.fill(); };
      if (sc.body === 'giant') {  // ringed gas giant
        g.save(); g.translate(px, py); g.rotate(-0.6 + R() * 1.2);
        g.strokeStyle = `rgba(${sc.ring},.35)`; g.lineWidth = 10; g.beginPath(); g.ellipse(0, 0, pr * 2, pr * 0.45, 0, Math.PI, Math.PI * 2); g.stroke();
        ball(0, 0, pr, sc.planet);
        g.save(); g.beginPath(); g.arc(0, 0, pr, 0, 7); g.clip();
        g.fillStyle = 'rgba(90,40,20,.25)'; for (let i = -3; i < 4; i++) g.fillRect(-pr, i * pr * 0.22, pr * 2, pr * 0.075);
        g.restore();
        g.strokeStyle = `rgba(${sc.ring},.55)`; g.lineWidth = 10; g.beginPath(); g.ellipse(0, 0, pr * 2, pr * 0.45, 0, 0, Math.PI); g.stroke();
        g.restore();
      } else if (sc.body === 'ice' || sc.body === 'earth') {  // a lit planet: oceans, land, clouds, night side, atmosphere
        const earth = sc.body === 'earth', lx = -0.55, ly = -0.45;  // light from the upper left
        glow(px, py, pr * 1.5, earth ? 'rgba(80,160,255,.28)' : 'rgba(150,230,255,.28)');
        ball(px, py, pr, sc.planet);
        g.save(); g.beginPath(); g.arc(px, py, pr, 0, 7); g.clip();
        const blob = (cx, cy, r, pts = 18) => {  // an irregular closed shape (coastline, cloud bank)
          const ph = [R() * 6, R() * 6, R() * 6];
          g.beginPath();
          for (let i = 0; i <= pts; i++) {
            const a = i / pts * Math.PI * 2;
            const k = 1 + 0.28 * Math.sin(a * 3 + ph[0]) + 0.16 * Math.sin(a * 5 + ph[1]) + 0.1 * Math.sin(a * 9 + ph[2]);
            const x = cx + Math.cos(a) * r * k, y = cy + Math.sin(a) * r * k * 0.8;
            i ? g.lineTo(x, y) : g.moveTo(x, y);
          }
          g.closePath();
        };
        if (earth) {  // continents: green lowlands with tan highlands, plus islands
          for (let c = 0; c < 4; c++) {
            const cx = px + (R() - 0.5) * pr * 1.4, cy = py + (R() - 0.5) * pr * 1.3, r = pr * (0.18 + R() * 0.2);
            const land = g.createRadialGradient(cx, cy, r * 0.1, cx, cy, r * 1.3);
            land.addColorStop(0, '#b8a36a'); land.addColorStop(0.45, '#5f8f43'); land.addColorStop(1, '#2f6a35');
            g.fillStyle = land; blob(cx, cy, r); g.fill();
            for (let i = 0; i < 4; i++) { g.fillStyle = '#3f7a3a'; blob(cx + (R() - 0.5) * r * 2.6, cy + (R() - 0.5) * r * 2.2, r * (0.08 + R() * 0.12), 12); g.fill(); }
          }
          g.filter = 'blur(1.5px)'; g.fillStyle = 'rgba(240,248,255,.8)'; blob(px + pr * 0.1, py - pr * 1.0, pr * 0.26, 20); g.fill(); blob(px - pr * 0.05, py + pr * 1.02, pr * 0.2, 20); g.fill(); g.filter = 'none';  // ice caps
        }
        // clouds: soft streaks that follow the planet's curve
        g.filter = 'blur(3px)';
        for (let i = 0; i < 9; i++) {
          const cy = py + (R() - 0.5) * pr * 1.7, half = Math.sqrt(Math.max(0, pr * pr - (cy - py) ** 2));
          g.strokeStyle = `rgba(255,255,255,${0.22 + R() * 0.3})`; g.lineWidth = pr * (0.03 + R() * 0.06);
          g.beginPath(); const x0 = px - half * (0.2 + R() * 0.7), x1 = x0 + half * (0.5 + R() * 0.9);
          g.moveTo(x0, cy); g.quadraticCurveTo((x0 + x1) / 2, cy - pr * (0.05 + R() * 0.1), x1, cy + pr * (R() - 0.5) * 0.1); g.stroke();
        }
        for (let i = 0; i < 3; i++) { g.fillStyle = 'rgba(255,255,255,.25)'; blob(px + (R() - 0.5) * pr * 1.2, py + (R() - 0.5) * pr * 1.2, pr * (0.12 + R() * 0.1), 14); g.fill(); }
        g.filter = 'none';
        // night side
        const night = g.createRadialGradient(px + lx * pr * 0.9, py + ly * pr * 0.9, pr * 0.2, px + lx * pr * 0.5, py + ly * pr * 0.5, pr * 1.9);
        night.addColorStop(0, 'rgba(0,0,10,0)'); night.addColorStop(0.42, 'rgba(0,0,12,.1)'); night.addColorStop(0.62, 'rgba(0,2,14,.7)'); night.addColorStop(0.8, 'rgba(0,2,14,.93)'); night.addColorStop(1, 'rgba(0,2,14,.97)');
        g.fillStyle = night; g.fillRect(px - pr, py - pr, pr * 2, pr * 2);
        g.restore();
        // thin atmosphere on the lit limb
        const rim = g.createRadialGradient(px, py, pr * 0.94, px, py, pr * 1.07);
        rim.addColorStop(0, 'rgba(120,190,255,0)'); rim.addColorStop(0.45, earth ? 'rgba(120,190,255,.55)' : 'rgba(190,240,255,.5)'); rim.addColorStop(1, 'rgba(120,190,255,0)');
        g.save(); g.fillStyle = rim; g.beginPath(); g.arc(px, py, pr * 1.07, 0, 7); g.fill(); g.restore();
      } else if (sc.body === 'sun' || sc.body === 'binary') {  // one star, or two
        const one = (x, y, r) => { glow(x, y, r * 4, `rgba(${sc.sun[0]},.45)`); glow(x, y, r * 1.6, `rgba(${sc.sun[1]},.9)`); ball(x, y, r, ['#ffffff', `rgb(${sc.sun[1]})`, `rgb(${sc.sun[0]})`]); };
        one(px, py, pr * 0.6);
        if (sc.body === 'binary') one(px + pr * 1.6, py - pr * 0.7, pr * 0.35);
      } else if (sc.body === 'hole') {  // a singularity: lensed accretion disk around a black shadow
        const r = pr * 0.75, warm = (a) => `rgba(255,${206 + Math.round(R() * 30)},${150 + Math.round(R() * 50)},${a})`;
        g.save(); g.translate(px, py); g.rotate((R() - 0.5) * 0.25);
        const halo = (rad, c) => { const n = g.createRadialGradient(0, 0, 0, 0, 0, rad); n.addColorStop(0, c); n.addColorStop(1, 'rgba(0,0,0,0)'); g.fillStyle = n; g.fillRect(-rad, -rad, rad * 2, rad * 2); };
        halo(r * 4.5, 'rgba(255,190,130,.08)');
        // the flare where the bent light meets the disk
        g.save(); g.scale(2.6, 1); halo(r * 1.5, 'rgba(255,214,170,.22)'); g.restore();
        // the far side of the disk, bent over the top (bright, wide) and under the bottom (narrower)
        const arcBand = (r0, r1, a0, a1, peak) => {
          const n = g.createRadialGradient(0, 0, r0, 0, 0, r1);
          n.addColorStop(0, `rgba(255,250,240,${peak})`); n.addColorStop(0.12, `rgba(255,232,200,${peak * 0.85})`);
          n.addColorStop(0.45, `rgba(255,205,150,${peak * 0.4})`); n.addColorStop(1, 'rgba(255,190,130,0)');
          g.fillStyle = n; g.beginPath(); g.arc(0, 0, r1, a0, a1); g.arc(0, 0, r0, a1, a0, true); g.closePath(); g.fill();
        };
        arcBand(r * 0.92, r * 1.75, Math.PI, Math.PI * 2, 0.95);
        arcBand(r * 0.92, r * 1.4, 0, Math.PI, 0.7);
        for (let i = 0; i < 700; i++) {  // fine streaks give it the swirling texture
          const top = R() < 0.62, a0 = top ? Math.PI + R() * Math.PI : R() * Math.PI;
          const rr = r * (0.95 + R() ** 1.8 * (top ? 0.8 : 0.45)), len = 0.08 + R() * 0.45;
          g.strokeStyle = warm((top ? 0.32 : 0.22) * R()); g.lineWidth = 0.5 + R() * 1.2;
          g.beginPath(); g.arc(0, 0, rr, a0, Math.min(a0 + len, top ? Math.PI * 2 : Math.PI)); g.stroke();
        }
        // the shadow and its photon ring
        g.fillStyle = '#000'; g.beginPath(); g.arc(0, 0, r * 0.86, 0, 7); g.fill();
        g.shadowColor = 'rgba(255,240,220,.9)'; g.shadowBlur = 10;
        g.strokeStyle = 'rgba(255,250,240,.95)'; g.lineWidth = 2.2; g.beginPath(); g.arc(0, 0, r * 0.9, 0, 7); g.stroke();
        g.shadowBlur = 0;
        // the near side of the disk, edge-on, crossing in front of the shadow
        for (let j = 0; j < 46; j++) {
          const a = 0.32 * (1 - j / 46) ** 1.3;
          g.strokeStyle = warm(a); g.lineWidth = 1.2;
          g.beginPath(); g.ellipse(0, r * 0.02, r * (0.95 + j * 0.13), r * (0.035 + j * 0.0032), 0, 0, Math.PI * 2); g.stroke();
        }
        const band = g.createLinearGradient(-r * 7, 0, r * 7, 0);
        band.addColorStop(0, 'rgba(255,220,180,0)'); band.addColorStop(0.32, 'rgba(255,220,180,.35)'); band.addColorStop(0.5, 'rgba(255,244,228,.95)');
        band.addColorStop(0.68, 'rgba(255,220,180,.35)'); band.addColorStop(1, 'rgba(255,220,180,0)');
        g.fillStyle = band; g.fillRect(-r * 7, -r * 0.035, r * 14, r * 0.07);
        for (let i = 0; i < 2600; i++) {  // dust in the disk
          const x = (R() - 0.5) * r * 13 * Math.sqrt(R()), y = (R() + R() + R() - 1.5) * r * 0.09 * (1 + Math.abs(x) / (r * 6));
          g.fillStyle = warm(0.12 + R() * 0.45 * (1 - Math.abs(x) / (r * 6.5)));
          g.fillRect(x, y, 1 + R() * 2.5, 1);
        }
        g.restore();
      }
      if (sc.moon) ball(px + pr * (1.5 + R()), py - pr * (0.8 + R() * 0.5), pr * 0.22, ['#d9d4cc', '#7d776e', '#1c1a17']);
    });
  }

  let spaceNo = sceneNo(fromUrl >= 1 ? fromUrl : opts.space);
  const background = galaxyTex(spaceNo);
  // a new background texture: the scene numbered n (1..6), or a random other one for 'random'
  function space(want) {
    let n = sceneNo(want);
    if (!(parseInt(want, 10) >= 1)) while (n === spaceNo && SCENES.length > 1) n = sceneNo();
    spaceNo = n;
    return galaxyTex(n);
  }
  return { load, makeRobot, makeStation, decorate, bridge, background, pod, space, scenes: SCENES.map(s => s.name), spaceNo: () => spaceNo, planets: () => planets.filter(p => p.parent && p.parent.parent), thrusting: () => engines.length > 0, thrust, accent, statusLed, NO_LINE };
}
