// Glitter for the full-screen view (off by default; tray → Glitter).
// Fine twinkling dust plus a few four-point sparkles drifting slowly over the
// background, under the glass card and the lyrics. They twinkle with the
// music and flare on strong beats.
// Colour modes: 'white' (default when on), 'multi' (rainbow), 'album' (cover colours).
window.Glitter = (() => {
  const canvas = document.getElementById('glitter');
  if (!canvas) return null;
  const ctx = canvas.getContext('2d');
  let mode = 'off';
  let palette = [[255, 255, 255]];
  let parts = [];
  let sprites = [];     // per colour: [dot, star]
  let dpr = 1, W = 0, H = 0, lastT = 0, slow = 0, flash = 0;

  const rand = (a, b) => a + Math.random() * (b - a);

  function hsl(h, s, l) {
    const k = (n) => (n + h * 12) % 12;
    const a = s * Math.min(l, 1 - l);
    const f = (n) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
    return [f(0) * 255, f(8) * 255, f(4) * 255].map(Math.round);
  }

  function colours() {
    if (mode === 'multi') return Array.from({ length: 8 }, (_, i) => hsl(i / 8, 0.9, 0.68));
    if (mode === 'album') return palette.length ? palette : [[255, 255, 255]];
    return [[255, 255, 255], [255, 247, 232]]; // white (+ a warm white)
  }

  // pre-rendered glowing sprites per colour – cheap to draw hundreds per frame
  function makeSprites() {
    sprites = colours().map(([r, g, b]) => {
      const S = 64;
      const dot = document.createElement('canvas');
      dot.width = dot.height = S;
      let x = dot.getContext('2d');
      let gr = x.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
      gr.addColorStop(0, 'rgba(255,255,255,1)');
      gr.addColorStop(0.12, `rgba(${r},${g},${b},1)`);
      gr.addColorStop(0.35, `rgba(${r},${g},${b},.35)`);
      gr.addColorStop(1, `rgba(${r},${g},${b},0)`);
      x.fillStyle = gr;
      x.fillRect(0, 0, S, S);

      const star = document.createElement('canvas');
      star.width = star.height = S * 2;
      x = star.getContext('2d');
      const c = S;
      gr = x.createRadialGradient(c, c, 0, c, c, S * 0.45);
      gr.addColorStop(0, `rgba(${r},${g},${b},.55)`);
      gr.addColorStop(1, `rgba(${r},${g},${b},0)`);
      x.fillStyle = gr;
      x.fillRect(0, 0, S * 2, S * 2);
      // long thin rays
      for (const [dx, dy, len] of [[1, 0, 1], [0, 1, 1], [0.7071, 0.7071, 0.45], [0.7071, -0.7071, 0.45]]) {
        const lg = x.createLinearGradient(c - dx * S * len, c - dy * S * len, c + dx * S * len, c + dy * S * len);
        lg.addColorStop(0, `rgba(${r},${g},${b},0)`);
        lg.addColorStop(0.5, 'rgba(255,255,255,1)');
        lg.addColorStop(1, `rgba(${r},${g},${b},0)`);
        x.strokeStyle = lg;
        x.lineWidth = len > 0.5 ? 2.2 : 1.4;
        x.beginPath();
        x.moveTo(c - dx * S * len, c - dy * S * len);
        x.lineTo(c + dx * S * len, c + dy * S * len);
        x.stroke();
      }
      gr = x.createRadialGradient(c, c, 0, c, c, 6);
      gr.addColorStop(0, 'rgba(255,255,255,1)');
      gr.addColorStop(1, 'rgba(255,255,255,0)');
      x.fillStyle = gr;
      x.fillRect(c - 8, c - 8, 16, 16);
      return [dot, star];
    });
    parts.forEach((p) => { p.ci = Math.floor(Math.random() * sprites.length); });
  }

  function spawn(p, anywhere) {
    p.x = rand(0, W);
    p.y = anywhere ? rand(0, H) : rand(-0.05 * H, 0);
    p.star = Math.random() < 0.09;
    p.size = p.star ? rand(9, 20) : rand(1.2, 3.6) * (Math.random() < 0.12 ? 1.8 : 1);
    p.vy = rand(4, 16) * (p.star ? 0.6 : 1);          // css px / s, slowly falling
    p.sway = rand(6, 22);
    p.swayF = rand(0.15, 0.5);
    p.ph = rand(0, Math.PI * 2);
    p.tw = rand(0.8, 3.2);                              // twinkle speed
    p.base = rand(0.15, 0.6);
    p.rot = rand(0, Math.PI);
    p.ci = Math.floor(Math.random() * sprites.length);
    return p;
  }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    W = innerWidth; H = innerHeight;
    canvas.width = Math.max(2, Math.round(W * dpr));
    canvas.height = Math.max(2, Math.round(H * dpr));
    const n = Math.round(Math.min(420, Math.max(90, (W * H) / 7500)));
    parts = Array.from({ length: n }, () => spawn({}, true));
  }

  function setMode(m) {
    const next = ['white', 'multi', 'album'].includes(m) ? m : 'off';
    if (next === mode) return;
    mode = next;
    canvas.style.opacity = mode === 'off' ? '0' : '1';
    if (mode !== 'off') { makeSprites(); if (!parts.length) resize(); }
    else ctx.clearRect(0, 0, canvas.width, canvas.height);
  }

  function setPalette(list) {
    palette = (list && list.length ? list : [[255, 255, 255]]).map((c) => c.map(Math.round));
    if (mode === 'album') makeSprites();
  }

  function draw(tMs, level, playing) {
    if (mode === 'off') return;
    const t = tMs / 1000;
    const dt = Math.min(0.1, lastT ? t - lastT : 0.016);
    lastT = t;
    const lv = playing ? level : 0;
    slow += (lv - slow) * 0.06;
    if (playing && lv - slow > 0.1) flash = Math.min(1, flash + 0.6);   // beat → sparkle burst
    flash *= Math.pow(0.05, dt);

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.globalCompositeOperation = 'lighter';
    const tw = 1 + lv * 1.8;
    for (const p of parts) {
      p.y += p.vy * dt * (0.6 + lv * 0.8);
      if (p.y > H + 30) spawn(p, false);
      const x = p.x + Math.sin(t * p.swayF + p.ph) * p.sway;
      const s1 = Math.sin(t * p.tw * tw + p.ph);
      let a = p.base * (0.35 + 0.65 * Math.max(0, s1)) * (0.55 + lv * 0.9) + flash * (p.star ? 0.6 : 0.35) * Math.max(0, s1);
      a = Math.min(1, a);
      if (a < 0.02) continue;
      const [dot, star] = sprites[p.ci] || sprites[0];
      ctx.globalAlpha = a;
      if (p.star) {
        const sz = p.size * (0.75 + 0.5 * Math.max(0, s1) + flash * 0.4);
        ctx.save();
        ctx.translate(x, p.y);
        ctx.rotate(p.rot + t * 0.15);
        ctx.drawImage(star, -sz, -sz, sz * 2, sz * 2);
        ctx.restore();
      } else {
        const sz = p.size * 4 * (0.8 + 0.4 * Math.max(0, s1));
        ctx.drawImage(dot, x - sz / 2, p.y - sz / 2, sz, sz);
      }
    }
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
  }

  resize();
  return { setMode, setPalette, draw, resize };
})();
