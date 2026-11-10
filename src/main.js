// NeedleX – island UI
// Outside the app (plain browser) a demo bridge lets the island be previewed:
// index.html?view=expanded|lyrics|peek
const DEMO = !window.__TAURI__ ? new URLSearchParams(location.search) : null;
const { invoke, listen } = DEMO ? islandDemo(DEMO) : { invoke: window.__TAURI__.core.invoke, listen: window.__TAURI__.event.listen };

function islandDemo(q) {
  const h = {};
  const mk = (w, hh, paint) => { const c = document.createElement('canvas'); c.width = w; c.height = hh; paint(c.getContext('2d'), w, hh); return c.toDataURL('image/jpeg', 0.9); };
  // a busy fake desktop so the refraction is visible
  const desk = mk(560, 520, (g, w, hh) => {
    const gr = g.createLinearGradient(0, 0, w, hh); gr.addColorStop(0, '#2b6cb0'); gr.addColorStop(0.5, '#d53f8c'); gr.addColorStop(1, '#f6ad55');
    g.fillStyle = gr; g.fillRect(0, 0, w, hh);
    g.fillStyle = 'rgba(255,255,255,.9)'; g.font = 'bold 22px Segoe UI';
    for (let y = 30; y < hh; y += 34) g.fillText('Liquid glass refracts the desktop behind it  •  NeedleX', 6 - (y % 60), y);
  });
  const art = mk(300, 300, (g) => { const gr = g.createLinearGradient(0, 0, 300, 300); gr.addColorStop(0, '#1e3a8a'); gr.addColorStop(1, '#0b0b1a'); g.fillStyle = gr; g.fillRect(0, 0, 300, 300); g.fillStyle = '#e11d48'; g.fillRect(40, 60, 220, 70); });
  document.body.style.background = `url(${desk}) 0 0 / 100% 100%`;
  setTimeout(() => {
    h['settings']?.({ payload: { floating: false, liveLyrics: true, hideIdle: true, glow: true } });
    h['media:track']?.({ payload: { key: 'd', title: 'All I Know', artist: 'The Weeknd', album: 'Starboy', art, source: 'demo', sourceName: 'Spotify' } });
    h['media:state']?.({ payload: { active: true, playing: true, positionMs: 61000, durationMs: 321000, hasTimeline: true, canNext: true, canPrev: true, canSeek: true } });
    const v = q.get('view') || 'expanded';
    setTimeout(() => {
      if (v === 'lyrics') { S.lyricsOpen = true; }
      if (v !== 'peek') { S.peekUntil = 0; h['island:hover']?.({ payload: true }); }
    }, 300);
  }, 50);
  return {
    invoke: async (cmd) => {
      if (cmd === 'capture_backdrop') return desk;
      if (cmd === 'fetch_lyrics') return { found: true, instrumental: false, plain: null, synced: [{ time: 58000, text: 'All I know is this' }, { time: 62000, text: 'all I know, all I know' }, { time: 66000, text: 'Every time I ride with you' }] };
      if (cmd === 'get_settings') return { floating: false, liveLyrics: true, hideIdle: true, glow: true };
      return null;
    },
    listen: (ev, fn) => { h[ev] = fn; },
  };
}

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const lsGet = (k) => { try { return localStorage.getItem(k); } catch { return null; } };
const lsSet = (k, v) => { try { localStorage.setItem(k, v); } catch {} };

const island = $('#island');
const snapImg = $('.glass .snap');
LiquidGlass.ensure('lg-island', { scale: 74, blur: 4.5, dispersion: 0.12 });
const GLASS_VIEWS = new Set(['peek', 'expanded', 'lyrics']);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Island geometry per view (CSS px). Window is 560 wide; island is centered.
const SIZES = {
  hidden:   { w: 200, h: 34, r: 17 },
  idle:     { w: 220, h: 34, r: 17 },
  compact:  { w: 340, h: 36, r: 18 },
  peek:     { w: 400, h: 74, r: 28 },
  expanded: { w: 460, h: 206, r: 34 },
  lyrics:   { w: 460, h: 482, r: 34 },
};
const LYRIC_LEAD_MS = 220; // show a line slightly before it's sung, feels in sync

const PLACEHOLDER = 'data:image/svg+xml;utf8,' + encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#3a3a44"/><stop offset="1" stop-color="#16161b"/></linearGradient></defs><rect width="100" height="100" fill="url(#g)"/><path d="M42 30v32.5a9 9 0 1 0 5 8V43l20-5v19.5a9 9 0 1 0 5 8V24z" fill="#ffffff" fill-opacity=".55"/></svg>`);

const S = {
  track: null,
  play: { active: false, playing: false, positionMs: 0, durationMs: 0, hasTimeline: false, canNext: true, canPrev: true, canSeek: false },
  playAt: performance.now(),
  localPos: 0,
  level: 0,
  lastLevelAt: 0,
  gotLevel: false,
  settings: { floating: false, liveLyrics: true, hideIdle: true, glow: true },
  hover: false,
  hoverT: 0,
  peekUntil: 0,
  lyricsOpen: lsGet('lyricsOpen') === '1',
  lyrics: null,
  lyricsState: 'none',
  lyrIdx: -2,
  lyrT: 0,
  scrubbing: false,
  scrubPct: 0,
  seekUntil: 0,
  toggleUntil: 0,
  view: 'hidden',
  hit: '',
  hires: {},
  snapOk: false,
  snapping: null,
  wantHover: false,
};

// ------------------------------------------------------------ helpers

function pos() {
  const p = S.play;
  if (!p.hasTimeline) return S.localPos;
  const v = p.positionMs + (p.playing ? performance.now() - S.playAt : 0);
  return Math.max(0, Math.min(v, p.durationMs));
}

function fmt(ms) {
  if (!isFinite(ms) || ms < 0) ms = 0;
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

function setText(sel, text) { $$(sel).forEach((el) => { el.textContent = text; }); }

// ------------------------------------------------------------ colors

function rgb2hsl(r, g, b) {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0, s = 0; const l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h /= 6;
  }
  return [h, s, l];
}

function hsl2rgb(h, s, l) {
  if (s === 0) return [l * 255, l * 255, l * 255];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s, p = 2 * l - q;
  const f = (t) => {
    if (t < 0) t += 1; if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return [f(h + 1 / 3) * 255, f(h) * 255, f(h - 1 / 3) * 255];
}

function setAccent(rgb) {
  const [r, g, b] = rgb.map((v) => Math.round(v));
  const root = document.documentElement.style;
  root.setProperty('--accent', `rgb(${r}, ${g}, ${b})`);
  root.setProperty('--accent-rgb', `${r}, ${g}, ${b}`);
}

// Picks the most vivid, mid-bright color of the cover (like Apple's UI does).
function extractColor(src) {
  const img = new Image();
  img.onload = () => {
    const N = 40;
    const c = document.createElement('canvas');
    c.width = c.height = N;
    const x = c.getContext('2d', { willReadFrequently: true });
    x.drawImage(img, 0, 0, N, N);
    const d = x.getImageData(0, 0, N, N).data;
    let r = 0, g = 0, b = 0, w = 0;
    for (let i = 0; i < d.length; i += 4) {
      const [, s, l] = rgb2hsl(d[i], d[i + 1], d[i + 2]);
      const wt = s * s * Math.max(0, 1 - Math.abs(l - 0.55) * 1.8) + 0.0005;
      r += d[i] * wt; g += d[i + 1] * wt; b += d[i + 2] * wt; w += wt;
    }
    let [h, s, l] = rgb2hsl(r / w, g / w, b / w);
    if (w / (N * N) < 0.02) s = Math.min(s, 0.1); // greyscale cover → stay neutral
    else s = Math.max(s, 0.55);
    l = Math.min(Math.max(l, 0.6), 0.74);
    setAccent(hsl2rgb(h, s, l));
  };
  img.src = src;
}

// ------------------------------------------------------------ view / layout

function computeView() {
  if (!S.track) return S.hover || !S.settings.hideIdle ? 'idle' : 'hidden';
  if (S.hover) return S.lyricsOpen ? 'lyrics' : 'expanded';
  if (performance.now() < S.peekUntil) return 'peek';
  return 'compact';
}

function render() {
  const v = computeView();
  const prevView = S.view;
  S.view = v;
  const size = SIZES[v];
  const floating = S.settings.floating;
  const baseTop = floating ? 8 : 0;
  const top = v === 'hidden' ? -(size.h + 16) : baseTop;
  const r = floating && (v === 'compact' || v === 'idle' || v === 'hidden') ? size.h / 2 : size.r;

  island.style.setProperty('--w', size.w + 'px');
  island.style.setProperty('--h', size.h + 'px');
  island.style.setProperty('--r', r + 'px');
  island.style.setProperty('--top', top + 'px');
  island.dataset.view = v === 'lyrics' ? 'expanded' : v;
  island.classList.toggle('lyrics-open', v === 'lyrics');
  const glassy = GLASS_VIEWS.has(v) && S.snapOk;
  island.classList.toggle('glassy', glassy);
  if (glassy) LiquidGlass.size('lg-island', size.w, size.h, r, v === 'peek' ? 20 : 28);
  island.classList.toggle('playing', !!S.track && S.play.playing);
  island.classList.toggle('paused', !!S.track && !S.play.playing);
  document.body.classList.toggle('floating', floating);
  document.body.classList.toggle('no-glow', !S.settings.glow);
  $('[data-act=lyrics]').classList.toggle('on', S.lyricsOpen);
  $('[data-act=next]').disabled = !S.play.canNext;
  $('[data-act=prev]').disabled = !S.play.canPrev;
  $('.scrub').classList.toggle('disabled', !(S.play.canSeek && S.play.hasTimeline));

  // Region of the window that should receive the mouse (everything else is click-through).
  const cx = window.innerWidth / 2;
  let hit;
  if (v === 'hidden') hit = { x: cx - 130, y: 0, w: 260, h: 6 };
  else {
    const pad = floating ? 8 : 18; // classic shape has the little "ears"
    hit = { x: cx - size.w / 2 - pad, y: 0, w: size.w + pad * 2, h: top + size.h + 10 };
  }
  const key = JSON.stringify(hit);
  if (key !== S.hit) { S.hit = key; invoke('set_hit_rect', hit); }

  if (v !== prevView && (v === 'lyrics' || v === 'expanded')) {
    S.lyrIdx = -2;
    requestAnimationFrame(updateMarquee);
  }
  updateLiveVisibility();
}

function updateMarquee() {
  const box = $('.f-title');
  const span = box.firstElementChild;
  box.classList.remove('scrolling');
  const over = span.scrollWidth - box.clientWidth;
  if (over > 4) {
    box.style.setProperty('--mq-dist', -(over + 16) + 'px');
    box.style.setProperty('--mq-dur', Math.max(7, (over + 16) / 22) + 's');
    box.classList.add('scrolling');
  }
}

// ------------------------------------------------------------ track / state

function setTrack(t) {
  if (!t || !t.key) {
    S.track = null;
    clearTimeout(S.lyrT);
    S.lyrics = null; S.lyricsState = 'none'; liveText = null; setLive('');
    render();
    return;
  }
  const isNew = !S.track || S.track.key !== t.key;
  const artChanged = !S.track || S.track.art !== t.art || isNew;
  S.track = t;

  setText('.c-title, .p-title', t.title);
  setText('.c-artist, .p-artist, .f-artist', t.artist || t.album || '');
  $('.c-sep').style.display = t.artist ? '' : 'none';
  $('.f-title span').textContent = t.title;
  $('.src-name').textContent = t.sourceName + (t.album && t.album !== t.title ? ` · ${t.album}` : '');
  requestAnimationFrame(updateMarquee);

  // once the high-resolution cover is in, ignore the small player thumbnail
  if (artChanged && !S.hires[t.key]) showArt(t.art || PLACEHOLDER, !!t.art);

  if (isNew) {
    S.localPos = 0;
    scheduleLyrics(t);
    loadHiresArt(t);
    if (!S.hover) {
      // glass needs a fresh look at the desktop before the peek grows out
      snapBackdrop().then(() => {
        if (!S.track || S.track.key !== t.key) return;
        S.peekUntil = performance.now() + 3800;
        render();
        setTimeout(render, 3850);
      });
    }
  }
  render();
}

function showArt(src, real) {
  $$('.art-img').forEach((i) => { i.src = src; });
  $('.tint-art').src = src;
  if (real) extractColor(src); else setAccent([235, 235, 240]);
}

async function loadHiresArt(t) {
  try {
    const url = await invoke('hires_art', { title: t.title, artist: t.artist || '', album: t.album || '' });
    if (!url || !S.track || S.track.key !== t.key) return;
    S.hires[t.key] = url;
    showArt(url, true);
  } catch {}
}

// Snapshot of the desktop under the island, refracted by the liquid glass.
async function snapBackdrop() {
  if (S.snapping) return S.snapping;
  S.snapping = (async () => {
    document.body.classList.add('snapping'); // keep our own overlays out of the shot
    await sleep(50);
    try {
      const url = await invoke('capture_backdrop');
      await new Promise((res) => { snapImg.onload = res; snapImg.onerror = res; snapImg.src = url; });
      S.snapOk = true;
    } catch {
      S.snapOk = false;
    }
    document.body.classList.remove('snapping');
  })();
  await Promise.race([S.snapping, sleep(400)]);
  S.snapping = null;
}

function onState(p) {
  const now = performance.now();
  const predicted = pos();
  const prev = S.play;
  const next = { ...p };

  // Keep optimistic play/pause for a moment so the button doesn't flicker.
  if (now < S.toggleUntil && S.track) next.playing = prev.playing;

  if (next.hasTimeline && prev.hasTimeline) {
    const diff = Math.abs(predicted - next.positionMs);
    const smooth = next.playing && prev.playing && diff < 450; // tiny IPC jitter → keep our clock
    const seeking = now < S.seekUntil && diff > 450;          // player hasn't applied the seek yet
    if (smooth || seeking) next.positionMs = predicted;
  }
  const changed = prev.playing !== next.playing || prev.canNext !== next.canNext
    || prev.canPrev !== next.canPrev || prev.canSeek !== next.canSeek || prev.hasTimeline !== next.hasTimeline;
  S.play = next;
  S.playAt = now;
  if (changed) render();
}

// ------------------------------------------------------------ lyrics

function lyricStatus(msg) {
  $('.lyr-status').textContent = msg;
}

function scheduleLyrics(t) {
  clearTimeout(S.lyrT);
  S.lyrics = null;
  S.lyricsState = 'loading';
  liveText = null; setLive('');
  buildLyrics();
  // wait a moment so we know the song's duration (used to pick the right version)
  S.lyrT = setTimeout(() => fetchLyrics(t, 0), 700);
}

async function fetchLyrics(t, attempt) {
  if (!S.track || S.track.key !== t.key) return;
  try {
    const res = await invoke('fetch_lyrics', {
      title: t.title,
      artist: t.artist || '',
      durationMs: S.play.hasTimeline ? S.play.durationMs : 0,
    });
    if (!S.track || S.track.key !== t.key) return;
    S.lyrics = res;
    S.lyricsState = 'done';
  } catch (e) {
    if (attempt < 2) { S.lyrT = setTimeout(() => fetchLyrics(t, attempt + 1), 4000); return; }
    if (!S.track || S.track.key !== t.key) return;
    S.lyricsState = 'error';
  }
  buildLyrics();
}

function buildLyrics() {
  const inner = $('.lyr-inner');
  const box = $('.f-lyrics');
  inner.innerHTML = '';
  inner.style.transform = 'none';
  box.classList.remove('plain');
  box.scrollTop = 0;
  S.lyrIdx = -2;
  lyricStatus('');
  const L = S.lyrics;
  if (S.lyricsState === 'loading') return lyricStatus('Looking for lyrics…');
  if (S.lyricsState === 'error') return lyricStatus("Couldn't reach the lyrics service");
  if (!L || !L.found) return lyricStatus('No lyrics found for this song');
  if (L.instrumental && !L.synced.length && !L.plain) return lyricStatus('♪  Instrumental  ♪');
  if (L.synced.length) {
    const frag = document.createDocumentFragment();
    L.synced.forEach((ln) => {
      const d = document.createElement('div');
      d.className = 'lyr-line' + (ln.text ? '' : ' gap');
      d.textContent = ln.text || '• • •';
      d.addEventListener('click', () => seek(ln.time));
      frag.appendChild(d);
    });
    inner.appendChild(frag);
  } else {
    box.classList.add('plain');
    const d = document.createElement('div');
    d.className = 'lyr-line lyr-plain';
    d.textContent = L.plain;
    inner.appendChild(d);
  }
}

function lyricIndex(p) {
  const L = S.lyrics.synced;
  let lo = 0, hi = L.length - 1, ans = -1;
  while (lo <= hi) {
    const m = (lo + hi) >> 1;
    if (L[m].time <= p) { ans = m; lo = m + 1; } else hi = m - 1;
  }
  return ans;
}

function setActive(idx) {
  const lines = $('.lyr-inner').children;
  for (let i = 0; i < lines.length; i++) {
    lines[i].classList.toggle('active', i === idx);
    lines[i].classList.toggle('past', i < idx);
  }
  S.lyrIdx = idx;
  const el = lines[Math.max(0, idx)];
  if (el) {
    const y = el.offsetTop + el.offsetHeight / 2 - 270 * 0.42;
    $('.lyr-inner').style.transform = `translateY(${-Math.max(-80, y)}px)`;
  }
  const line = idx >= 0 ? S.lyrics.synced[idx] : null;
  setLive(line ? line.text : '');
}

// ------------------------------------------------------------ live lyric pill

const live = $('#live');
const livePill = $('.live-pill');
const liveSpans = [$('.live-a'), $('.live-b')];
let liveCur = 0;
let liveText = null;
const measureCtx = document.createElement('canvas').getContext('2d');

function setLive(text) {
  if (text === liveText) return;
  liveText = text;
  const out = liveSpans[liveCur];
  liveCur ^= 1;
  const inn = liveSpans[liveCur];
  out.classList.add('out');
  inn.classList.remove('out');
  inn.textContent = text;
  inn.style.setProperty('--p', '0%');
  inn.classList.add('in-start');
  void inn.offsetWidth;
  inn.classList.remove('in-start');
  if (text) {
    measureCtx.font = `650 14px ${getComputedStyle(document.body).fontFamily}`;
    const w = Math.min(520, Math.max(110, measureCtx.measureText(text).width + 40));
    livePill.style.width = w + 'px';
  }
  updateLiveVisibility();
}

function updateLiveVisibility() {
  const show = S.settings.liveLyrics && !!liveText && !!S.track && S.play.playing && S.view === 'compact';
  live.classList.toggle('show', show);
  live.style.top = ((S.settings.floating ? 8 : 0) + SIZES.compact.h + 8) + 'px';
}

// ------------------------------------------------------------ controls

function seek(ms) {
  if (!S.track) return;
  invoke('media_seek', { ms });
  S.play = { ...S.play, positionMs: ms };
  S.playAt = performance.now();
  S.seekUntil = performance.now() + 1500;
  if (!S.play.hasTimeline) S.localPos = ms;
  S.lyrIdx = -2;
}

$$('[data-act]').forEach((b) => b.addEventListener('click', (e) => {
  e.stopPropagation();
  const act = b.dataset.act;
  if (act === 'lyrics') {
    S.lyricsOpen = !S.lyricsOpen;
    lsSet('lyricsOpen', S.lyricsOpen ? '1' : '0');
    render();
    return;
  }
  if (act === 'hide') {
    // hide the notch completely; bring it back from the tray icon
    clearTimeout(S.hoverT);
    S.hover = false;
    render();
    setTimeout(() => invoke('set_notch_hidden', { hidden: true }), 260);
    return;
  }
  if (act === 'toggle') {
    const p = pos();
    S.play = { ...S.play, playing: !S.play.playing, positionMs: p };
    S.playAt = performance.now();
    S.toggleUntil = performance.now() + 1000;
    render();
  }
  invoke('media_control', { action: act });
}));

const scrub = $('.scrub');
const scrubPct = (e) => {
  const r = scrub.getBoundingClientRect();
  return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
};
scrub.addEventListener('pointerdown', (e) => {
  if (!(S.play.canSeek && S.play.hasTimeline)) return;
  S.scrubbing = true;
  S.scrubPct = scrubPct(e);
  scrub.classList.add('dragging');
  scrub.setPointerCapture(e.pointerId);
});
scrub.addEventListener('pointermove', (e) => { if (S.scrubbing) S.scrubPct = scrubPct(e); });
const endScrub = () => {
  if (!S.scrubbing) return;
  S.scrubbing = false;
  scrub.classList.remove('dragging');
  seek(S.scrubPct * S.play.durationMs);
};
scrub.addEventListener('pointerup', endScrub);
scrub.addEventListener('pointercancel', endScrub);

// clicking the album art (in any island state) opens the full-screen view
$$('.c-art, .p-art, .f-art').forEach((el) => el.addEventListener('click', (e) => {
  e.stopPropagation();
  if (!S.track) return;
  clearTimeout(S.hoverT);
  S.hover = false;
  render();
  invoke('open_full');
}));

// clicking the compact pill opens it (in case hover-to-open is too subtle)
$('.l-compact').addEventListener('click', () => { if (S.justDragged) return; S.hover = true; render(); });
$('.l-peek').addEventListener('click', () => { S.hover = true; render(); });

// ------------------------------------------------------------ drag the notch sideways
// Point at the left/right edge of the notch: grips appear, then drag it.
// Near the middle it snaps back to the centre like a magnet.
const centerGuide = $('#center-guide');
function edgeZone(x) {
  if (S.view !== 'compact' && S.view !== 'idle') return false;
  const r = island.getBoundingClientRect();
  const pad = S.settings.floating ? 10 : 20;
  return (x >= r.left - pad && x <= r.left + 18) || (x >= r.right - 18 && x <= r.right + pad);
}
function setEdge(on) {
  if (on === S.edge) return;
  S.edge = on;
  island.classList.toggle('grip', on);
  document.body.style.cursor = on ? 'ew-resize' : '';
}
document.addEventListener('pointermove', (e) => {
  if (S.drag) return;
  setEdge(edgeZone(e.clientX));
  if (!S.edge && S.wantHover && !S.hover && !S.expandPending) scheduleExpand(40);
});
document.addEventListener('pointerdown', (e) => {
  if (!S.edge || e.button !== 0) return;
  e.preventDefault();
  e.stopPropagation();
  clearTimeout(S.hoverT);
  S.expandPending = false;
  const from = S.settings.notchOffset || 0;
  // the drag itself is tracked natively (real screen pixels, any DPI) until
  // the mouse button is released; we only show feedback from its events
  S.drag = { from, moved: false };
  S.wasSnapped = from === 0;
  island.setPointerCapture(e.pointerId);
  island.classList.add('dragging');
  document.body.style.cursor = 'grabbing';
  invoke('drag_notch_start');
}, true);
listen('notch:drag', (e) => {
  const { applied, want, done } = e.payload;
  if (done) { finishDrag(applied); return; }
  if (!S.drag) return;
  if (Math.abs(want - S.drag.from) > 3) S.drag.moved = true;
  magnetFeedback(applied === 0, want, applied);
});
listen('display', (e) => { S.zoom = e.payload || 1; S.hit = ''; render(); });
function magnetFeedback(snapped, want, applied) {
  // the real screen centre (offsets are screen px, the page may be zoomed)
  centerGuide.style.left = `calc(50% - ${applied / (S.zoom || 1)}px)`;
  if (snapped && !S.wasSnapped) {          // little squash when the magnet grabs it
    island.classList.remove('magnet');
    void island.offsetWidth;
    island.classList.add('magnet');
  }
  S.wasSnapped = snapped;
  centerGuide.classList.toggle('show', !!S.drag && Math.abs(want) < 150);
  centerGuide.classList.toggle('locked', snapped);
}
function finishDrag(applied) {
  const d = S.drag;
  S.drag = null;
  S.settings.notchOffset = applied;
  island.classList.remove('dragging');
  centerGuide.classList.remove('show', 'locked');
  document.body.style.cursor = S.edge ? 'ew-resize' : '';
  if (d && d.moved) { S.justDragged = true; setTimeout(() => { S.justDragged = false; }, 150); }
}

function scheduleExpand(delay) {
  clearTimeout(S.hoverT);
  S.expandPending = true;
  S.hoverT = setTimeout(async () => {
    S.expandPending = false;
    if (S.edge || S.drag || !S.wantHover || S.hover) return;
    if (S.track) await snapBackdrop();
    if (!S.wantHover || S.edge || S.drag) return;
    S.hover = true;
    render();
  }, delay);
}

// ------------------------------------------------------------ animation loop

const barSets = { compact: $$('.c-bars i'), peek: $$('.p-bars i'), expanded: $$('.f-bars i'), lyrics: $$('.f-bars i') };
const glowEl = $('.glow');
let lastT = performance.now();
let smooth = 0;
let lastSec = -1;
let lastDurText = '';

function frame(t) {
  const dt = Math.min(100, t - lastT);
  lastT = t;
  const playing = !!S.track && S.play.playing;
  if (playing && !S.play.hasTimeline) S.localPos += dt;

  // audio level → bars + glow
  if (t - S.lastLevelAt > 120) S.level *= 0.85;
  let target = playing ? Math.min(1, Math.sqrt(S.level) * 1.12) : 0;
  if (playing && !S.gotLevel) target = 0.5 + 0.25 * Math.sin(t / 170) * Math.sin(t / 410); // no meter available
  smooth += (target - smooth) * (target > smooth ? 0.55 : 0.16);

  const bars = barSets[S.view];
  if (bars) {
    for (let i = 0; i < bars.length; i++) {
      const k = Math.abs(Math.sin(t * 0.0105 * (1 + i * 0.21) + i * 1.9) * 0.6 + Math.sin(t * 0.0043 * (1 + i * 0.37) + i) * 0.4);
      const h = playing ? Math.max(0.16, Math.min(1, smooth * (0.35 + 0.85 * k))) : 0.16;
      bars[i].style.transform = `scaleY(${h.toFixed(3)})`;
    }
  }
  const glow = S.settings.glow && playing && S.view !== 'hidden' ? Math.pow(smooth, 1.8) * 0.75 : 0;
  glowEl.style.opacity = glow.toFixed(3);

  // keep the refraction map glued to the island while it springs open/closed
  if (island.classList.contains('glassy')) {
    const w = island.offsetWidth, h = island.offsetHeight;
    if (w !== S.gw || h !== S.gh) { S.gw = w; S.gh = h; LiquidGlass.stretch('lg-island', w, h); }
  }

  if (S.track) {
    const p = pos();

    // progress
    if (S.view === 'expanded' || S.view === 'lyrics') {
      const dur = S.play.durationMs;
      const shown = S.scrubbing ? S.scrubPct * dur : p;
      const pct = S.play.hasTimeline && dur > 0 ? (shown / dur) * 100 : 0;
      $('.scrub-fill').style.width = pct + '%';
      $('.scrub-knob').style.left = pct + '%';
      const sec = Math.floor(shown / 1000);
      if (sec !== lastSec) { lastSec = sec; $('.t-cur').textContent = fmt(shown); }
      const durText = S.play.hasTimeline ? fmt(dur) : '--:--';
      if (durText !== lastDurText) { lastDurText = durText; $('.t-dur').textContent = durText; }
    }

    // synced lyrics
    if (S.lyrics && S.lyrics.synced && S.lyrics.synced.length) {
      const L = S.lyrics.synced;
      const lp = p + LYRIC_LEAD_MS;
      const idx = lyricIndex(lp);
      if (idx !== S.lyrIdx) setActive(idx);
      if (idx >= 0 && live.classList.contains('show')) {
        const start = L[idx].time;
        const end = idx + 1 < L.length ? L[idx + 1].time : start + 4000;
        const prog = Math.min(1, Math.max(0, (lp - start) / Math.max(400, (end - start) * 0.9)));
        liveSpans[liveCur].style.setProperty('--p', (prog * 100).toFixed(1) + '%');
      }
    }
  }

  requestAnimationFrame(frame);
}

// ------------------------------------------------------------ idle clock

function tickClock() {
  const d = new Date();
  $('.idle-clock').textContent = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}
setInterval(tickClock, 5000);
tickClock();

// ------------------------------------------------------------ wiring

function applySettings(s) {
  S.settings = { ...S.settings, ...s };
  render();
}

listen('media:track', (e) => setTrack(e.payload));
listen('media:state', (e) => onState(e.payload));
listen('audio:level', (e) => { S.level = Math.max(S.level * 0.6, e.payload); S.lastLevelAt = performance.now(); S.gotLevel = true; });
listen('settings', (e) => applySettings(e.payload));
listen('island:hover', (e) => {
  clearTimeout(S.hoverT);
  S.wantHover = e.payload;
  if (e.payload) {
    // short wait so a pointer heading for the notch's edge (to drag it) doesn't open it
    scheduleExpand(110);
  } else {
    S.expandPending = false;
    if (!S.drag) setEdge(false);
    S.hoverT = setTimeout(function leave() {
      if (S.scrubbing || S.drag) { S.hoverT = setTimeout(leave, 200); return; }
      S.hover = false;
      render();
    }, 260);
  }
});

$$('.art-img').forEach((i) => { i.src = PLACEHOLDER; });
invoke('get_settings').then(applySettings).catch(() => render());
invoke('media_resend');
render();
requestAnimationFrame(frame);
