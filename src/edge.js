// Screen-edge light ("beat glow"), as in the reel:
//  * a thin hot line runs around the WHOLE screen edge and pulses with the music
//  * brighter sections keep circling around the edge – faster and longer when
//    the music is loud – and strong beats fire a short comet that races around
//  * only a soft, subdued glow spreads inward: the line is the star, not the haze
window.EdgeLight = (() => {
  const canvas = document.getElementById('edge-gl');
  const gl = canvas && canvas.getContext('webgl2', { alpha: true, premultipliedAlpha: true, antialias: false });
  if (!gl) return null;

  const N = 6; // moving light sections (2 circling + up to 4 comets)
  const VS = `#version 300 es
  in vec2 p; void main(){ gl_Position = vec4(p, 0.0, 1.0); }`;
  const FS = `#version 300 es
  precision highp float;
  out vec4 o;
  uniform vec2 uRes;
  uniform float uDpr, uRadius, uLevel, uBase;
  uniform vec3 uColor;
  uniform vec3 uSeg[${N}];   // x = centre (0..1 of perimeter), y = length (0..1), z = brightness

  float segI(float u){
    float I = 0.0;
    for (int i = 0; i < ${N}; i++){
      vec3 s = uSeg[i];
      if (s.z <= 0.001 || s.y <= 0.0005) continue;
      float du = abs(fract(u - s.x + 0.5) - 0.5);        // circular distance
      float hl = s.y * 0.5;
      float a = 1.0 - smoothstep(hl * 0.15, hl, du);     // tapered tail
      I += a * a * s.z;
    }
    return I;
  }

  // one edge: d = css px from that edge, u = position along the perimeter
  vec2 edge(float d, float u){
    float I = uBase + segI(u);                           // whole-edge pulse + moving sections
    float line = exp(-d / 1.7);                          // the hot line (~2-3 px)
    float halo = exp(-d / 5.0) * 0.32;                   // tight halo around it
    float glow = exp(-d / 26.0) * (0.06 + uLevel * 0.12);// subdued inward glow
    return vec2(line * I, (halo + glow) * I);
  }

  void main(){
    vec2 q = gl_FragCoord.xy;            // origin bottom-left
    float W = uRes.x, H = uRes.y, R = uRadius * uDpr;
    vec2 c = clamp(q, vec2(R), vec2(W - R, H - R));
    if (length(q - c) > R) { o = vec4(0.0); return; }   // outside the rounded corners
    float x = q.x, yT = H - q.y;
    if (min(min(x, W - x), min(yT, H - yT)) > 120.0 * uDpr) { o = vec4(0.0); return; }

    // perimeter runs clockwise from the top-left corner; every edge lights the
    // pixel on its own, so corners blend smoothly
    float P = 2.0 * (W + H);
    vec2 e = edge(yT / uDpr, x / P)
           + edge((W - x) / uDpr, (W + yT) / P)
           + edge(q.y / uDpr, (W + H + (W - x)) / P)
           + edge(x / uDpr, (2.0 * W + H + (H - yT)) / P);
    float line = clamp(e.x, 0.0, 1.0);
    float a = clamp(line + e.y * 0.8, 0.0, 1.0);
    if (a <= 0.002) { o = vec4(0.0); return; }
    vec3 col = mix(uColor, vec3(1.0), clamp(line * 0.9, 0.0, 1.0)); // white-hot core, tinted glow
    o = vec4(col * a, a);
  }`;

  function sh(t, src) {
    const s = gl.createShader(t);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) { console.error(gl.getShaderInfoLog(s)); return null; }
    return s;
  }
  const vs = sh(gl.VERTEX_SHADER, VS), fs = sh(gl.FRAGMENT_SHADER, FS);
  if (!vs || !fs) return null;
  const prog = gl.createProgram();
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) { console.error(gl.getProgramInfoLog(prog)); return null; }
  gl.useProgram(prog);
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, 'p');
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  const U = {};
  for (const n of ['uRes', 'uDpr', 'uRadius', 'uLevel', 'uBase', 'uColor', 'uSeg']) U[n] = gl.getUniformLocation(prog, n);

  // two sections chase each other around the edge (opposite sides)
  const runners = [{ c: 0.12, dir: 1 }, { c: 0.62, dir: 1 }];
  const comets = [];
  let slow = 0, lastBeat = 0, lastT = 0, punch = 0;

  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.max(2, Math.round(innerWidth * dpr));
    canvas.height = Math.max(2, Math.round(innerHeight * dpr));
    gl.viewport(0, 0, canvas.width, canvas.height);
  }

  // level 0..1 (music), playing, color [r,g,b] 0..255, strength 0..1
  function draw(tMs, level, playing, color, strength) {
    const t = tMs / 1000;
    const dt = Math.min(0.1, lastT ? t - lastT : 0.016);
    lastT = t;
    const lv = playing ? level : 0;

    // beat: an instant flash of the whole edge + a comet racing around
    slow += (lv - slow) * 0.06;
    const rise = lv - slow;
    if (playing && rise > 0.09 && t - lastBeat > 0.28) {
      lastBeat = t;
      punch = Math.min(1, 0.5 + rise * 3);
      const from = runners[Math.floor(Math.random() * runners.length)].c;
      comets.push({ c: from, v: (Math.random() < 0.5 ? -1 : 1) * (0.45 + Math.random() * 0.35), len: 0.04 + lv * 0.06, life: 1 });
      if (comets.length > 4) comets.shift();
    }
    punch *= Math.pow(0.02, dt); // fast decay (~0.25 s)

    const seg = new Float32Array(N * 3);
    // circling sections: always moving, faster/longer/brighter with the music
    const speed = 0.05 + lv * 0.22;
    runners.forEach((r, i) => {
      r.c = (r.c + r.dir * speed * dt + 1) % 1;
      const len = playing ? 0.14 + lv * 0.26 : 0.1;
      const br = playing ? 0.55 + lv * 1.1 + punch * 0.6 : 0.25;
      seg.set([r.c, len, br * strength], i * 3);
    });
    comets.forEach((cm, j) => {
      cm.c = (cm.c + cm.v * dt + 1) % 1;
      cm.life -= dt * 0.8;
      seg.set([cm.c, cm.len, Math.max(0, cm.life) * 1.4 * strength], (runners.length + j) * 3);
    });
    for (let i = comets.length - 1; i >= 0; i--) if (comets[i].life <= 0) comets.splice(i, 1);

    // the whole edge breathes with the music (quiet → faint rim, loud → bright rim)
    const base = (playing ? 0.1 + lv * 0.5 + punch * 0.45 : 0.06) * strength;

    // warm white, tinted a little by the cover
    const k = 0.35;
    const col = [
      (255 * (1 - k) + color[0] * k) / 255,
      (244 * (1 - k) + color[1] * k) / 255,
      (226 * (1 - k) + color[2] * k) / 255,
    ];

    gl.useProgram(prog);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(U.uRes, canvas.width, canvas.height);
    gl.uniform1f(U.uDpr, canvas.width / innerWidth);
    gl.uniform1f(U.uRadius, 12);
    gl.uniform1f(U.uLevel, lv);
    gl.uniform1f(U.uBase, base);
    gl.uniform3f(U.uColor, col[0], col[1], col[2]);
    gl.uniform3fv(U.uSeg, seg);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  resize();
  return { draw, resize };
})();
