// Screen-edge light ("beat glow"), as in the reel: a thin, very bright
// warm-white line hugging the screen edge with a soft inward bloom. Never the
// whole frame at once: a few lit sections with tapered ends slide around the
// edge, grow and flare with the music and shrink when it gets quiet; strong
// beats send a short bright comet running around the edge.
window.EdgeLight = (() => {
  const canvas = document.getElementById('edge-gl');
  const gl = canvas && canvas.getContext('webgl2', { alpha: true, premultipliedAlpha: true, antialias: false });
  if (!gl) return null;

  const VS = `#version 300 es
  in vec2 p; void main(){ gl_Position = vec4(p, 0.0, 1.0); }`;
  const N = 5; // light sections
  const FS = `#version 300 es
  precision highp float;
  out vec4 o;
  uniform vec2 uRes;
  uniform float uDpr, uRadius, uLevel, uTime;
  uniform vec3 uColor;
  uniform vec3 uSeg[${N}];   // x = centre (0..1 of perimeter), y = length (0..1), z = brightness

  // brightness of the light sections at perimeter position u (0..1)
  float segI(float u){
    float I = 0.0;
    for (int i = 0; i < ${N}; i++){
      vec3 s = uSeg[i];
      if (s.z <= 0.001 || s.y <= 0.0005) continue;
      float du = abs(fract(u - s.x + 0.5) - 0.5);        // circular distance
      float hl = s.y * 0.5;
      I += (1.0 - smoothstep(hl * 0.25, hl, du)) * s.z;  // tapered ends
    }
    return min(I, 1.6);
  }
  // light profile across the edge (d in css px from that edge)
  vec2 prof(float d){
    float core = exp(-d / 2.1);                           // the hot line itself (~3-4 px)
    float near = exp(-d / 7.5) * 0.62;                    // tight halo
    float bloom = exp(-d / 30.0) * (0.3 + uLevel * 0.35); // soft inward glow
    return vec2(core + near + bloom, core);
  }

  void main(){
    vec2 q = gl_FragCoord.xy;            // origin bottom-left
    float W = uRes.x, H = uRes.y, R = uRadius * uDpr;
    // rounded screen corners: nothing outside them
    vec2 c = clamp(q, vec2(R), vec2(W - R, H - R));
    if (length(q - c) > R) { o = vec4(0.0); return; }
    float band = 150.0 * uDpr;
    float x = q.x, yT = H - q.y;         // y measured from the top
    if (min(min(x, W - x), min(yT, H - yT)) > band) { o = vec4(0.0); return; }

    // every edge lights the pixel on its own (no seams in the corners);
    // perimeter runs clockwise from the top-left corner
    float P = 2.0 * (W + H);
    float a = 0.0, hot = 0.0;
    vec2 e;
    e = prof(yT / uDpr)      * segI(x / P);                      a += e.x; hot += e.y; // top
    e = prof((W - x) / uDpr) * segI((W + yT) / P);               a += e.x; hot += e.y; // right
    e = prof(q.y / uDpr)     * segI((W + H + (W - x)) / P);      a += e.x; hot += e.y; // bottom
    e = prof(x / uDpr)       * segI((2.0 * W + H + (H - yT)) / P); a += e.x; hot += e.y; // left
    a = clamp(a * 0.85, 0.0, 1.0);
    if (a <= 0.002) { o = vec4(0.0); return; }
    vec3 col = mix(uColor, vec3(1.0), clamp(hot * 0.85, 0.0, 1.0)); // white-hot core
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
  for (const n of ['uRes', 'uDpr', 'uRadius', 'uLevel', 'uTime', 'uColor', 'uSeg']) U[n] = gl.getUniformLocation(prog, n);

  // --- section dynamics
  // slow sections: drift around the edge, length/brightness follow the music
  const secs = [
    { c: 0.80, v: 0.012, base: 0.14, amp: 0.34, lag: 0.10, env: 0, ph: 0.0 },  // left/bottom
    { c: 0.30, v: -0.009, base: 0.09, amp: 0.30, lag: 0.06, env: 0, ph: 1.7 }, // right
    { c: 0.06, v: 0.006, base: 0.05, amp: 0.22, lag: 0.04, env: 0, ph: 3.1 },  // top
  ];
  // comets: short bright runs launched by strong beats
  const comets = [];
  let slow = 0, lastBeat = 0, lastT = 0;

  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.max(2, Math.round(innerWidth * dpr));
    canvas.height = Math.max(2, Math.round(innerHeight * dpr));
    gl.viewport(0, 0, canvas.width, canvas.height);
  }

  // level: 0..1 smoothed music level; playing: bool; color: [r,g,b] 0..255; strength 0..1
  function draw(tMs, level, playing, color, strength) {
    const t = tMs / 1000;
    const dt = Math.min(0.1, lastT ? t - lastT : 0.016);
    lastT = t;
    const lv = playing ? level : 0;

    // beat → comet running around the edge
    slow += (lv - slow) * 0.05;
    if (playing && lv - slow > 0.14 && t - lastBeat > 0.45) {
      lastBeat = t;
      const from = secs[Math.floor(Math.random() * secs.length)];
      comets.push({ c: from.c, v: (Math.random() < 0.5 ? -1 : 1) * (0.32 + Math.random() * 0.25), len: 0.05 + lv * 0.06, life: 1 });
      if (comets.length > 2) comets.shift();
    }

    const seg = new Float32Array(N * 3);
    secs.forEach((s, i) => {
      s.env += ((lv * (0.75 + 0.25 * Math.sin(t * 0.7 + s.ph))) - s.env) * (lv > s.env ? 0.35 : s.lag);
      s.c = (s.c + s.v * dt * (0.6 + lv * 1.6) + 1) % 1;
      const len = playing ? s.base + s.amp * s.env : 0.05;
      const br = playing ? 0.32 + 1.25 * s.env : 0.14;
      seg.set([s.c, len, br * strength], i * 3);
    });
    comets.forEach((cm, j) => {
      cm.c = (cm.c + cm.v * dt + 1) % 1;
      cm.life -= dt * 0.75;
      seg.set([cm.c, cm.len, Math.max(0, cm.life) * 1.3 * strength], (secs.length + j) * 3);
    });
    for (let i = comets.length - 1; i >= 0; i--) if (comets[i].life <= 0) comets.splice(i, 1);

    // warm white, tinted a little by the cover
    const k = 0.38;
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
    gl.uniform1f(U.uTime, t);
    gl.uniform3f(U.uColor, col[0], col[1], col[2]);
    gl.uniform3fv(U.uSeg, seg);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  resize();
  return { draw, resize };
})();
