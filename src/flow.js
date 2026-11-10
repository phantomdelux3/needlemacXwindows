// GPU background for the lyric views (WebGL2).
//  mode 0 "stars": the cover fills the screen and keeps flowing like liquid,
//                  radial zoom-blur streaks + horizontal smear lines toward the edges
//  mode 1 "words": blurred colour wash, the cover sharp on the right with its
//                  left edge stretched out into the wash
//  both: beat-driven glitch stripes, chromatic fringe, pixel-resolve on new art
window.FlowBG = (() => {
  const canvas = document.getElementById('bg-gl');
  const gl = canvas && canvas.getContext('webgl2', { antialias: false, alpha: false, premultipliedAlpha: false, preserveDrawingBuffer: false });
  if (!gl) return null;

  const VS = `#version 300 es
  in vec2 p; out vec2 vUv;
  void main(){ vUv = p * 0.5 + 0.5; gl_Position = vec4(p, 0.0, 1.0); }`;

  const FS = `#version 300 es
  precision highp float;
  in vec2 vUv; out vec4 o;
  uniform sampler2D uTex;
  uniform vec2 uRes;
  uniform float uImgAspect, uTime, uLevel, uMode, uPix, uGlitch, uSeed;
  uniform sampler2D uMask;

  float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7)) + uSeed) * 43758.5453); }
  float noise(vec2 p){
    vec2 i = floor(p), f = fract(p); vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1,0)), u.x), mix(hash(i + vec2(0,1)), hash(i + vec2(1,1)), u.x), u.y);
  }
  float fbm(vec2 p){ float v = 0.0, a = 0.5; for (int i = 0; i < 5; i++){ v += a * noise(p); p = p * 2.02 + 3.1; a *= 0.5; } return v; }

  // domain-warped flow field -> liquid movement
  vec2 flow(vec2 tc, float amt){
    float t = uTime * 0.055;
    vec2 q = vec2(fbm(tc * 2.0 + vec2(0.0, t)), fbm(tc * 2.0 + vec2(5.2, -t)));
    vec2 r = vec2(fbm(tc * 2.0 + 3.2 * q + vec2(1.7, 9.2) + t * 1.4), fbm(tc * 2.0 + 3.2 * q + vec2(8.3, 2.8) - t * 1.1));
    return tc + (r - 0.5) * amt;
  }
  vec2 crop(vec2 tc){                 // centre-square crop of non-square covers
    vec2 cs = uImgAspect > 1.0 ? vec2(1.0 / uImgAspect, 1.0) : vec2(1.0, uImgAspect);
    return 0.5 + (tc - 0.5) * cs;
  }
  vec3 tex(vec2 tc, float lod){ return textureLod(uTex, crop(clamp(tc, 0.002, 0.998)), lod).rgb; }
  vec2 pixelate(vec2 tc){ return uPix > 0.0 ? (floor(tc / uPix) + 0.5) * uPix : tc; }

  // smooth blur by averaging rotated taps from the mip level
  vec3 blurTex(vec2 tc, float lod){
    if (lod < 0.6) return tex(tc, 0.0);
    float px = exp2(lod) / float(textureSize(uTex, 0).y);
    vec3 a = tex(tc, lod);
    a += tex(tc + vec2( px,  px * 0.5), lod);
    a += tex(tc + vec2(-px * 0.5,  px), lod);
    a += tex(tc + vec2(-px, -px * 0.5), lod);
    a += tex(tc + vec2( px * 0.5, -px), lod);
    return a / 5.0;
  }

  vec3 wordsBG(vec2 uv, vec2 s, float aspect, float sharp, float stripe){
    float sz = max(1.3, aspect * 0.66);                // cover size in screen heights (wider on ultrawide)
    vec2 a = (s - vec2(0.16 * aspect, 0.03)) / sz + 0.5;
    a = flow(a, 0.01 + uLevel * 0.018);
    a = pixelate(a);
    if (stripe > 0.5) a.y = 0.5 + (a.y - 0.5) * 0.1;
    vec2 ac = clamp(a, 0.0, 1.0);
    float outside = length(a - ac);
    float df = length((s - vec2(0.13 * aspect, 0.02)) / vec2(0.42 * aspect, 0.55));
    float lod = smoothstep(0.38, 1.12, df) * 7.0;
    float edgeIn = min(min(a.x, 1.0 - a.x), min(a.y, 1.0 - a.y));          // distance to the cover edge
    lod = max(lod, 6.5 * smoothstep(0.16, 0.0, edgeIn));                    // dissolve toward the edge
    if (outside > 0.0) lod = max(lod, min(7.5, 6.5 + outside * 20.0));     // and stay dissolved past it
    lod = mix(min(lod, 7.5), 0.4, sharp);
    vec3 c = blurTex(ac, lod);
    float l = dot(c, vec3(0.299, 0.587, 0.114));
    c = mix(vec3(l), c, 1.0 + smoothstep(1.0, 5.0, lod) * 0.45);   // richer colour in the blur
    return c;
  }

  void main(){
    vec2 uv = vUv;
    float aspect = uRes.x / uRes.y;
    vec2 s = (uv - 0.5) * vec2(aspect, 1.0);
    // thin vertical glitch stripes, only for a moment on strong beats
    float col = floor(uv.x * 150.0);
    float stripe = step(1.0 - uGlitch * 0.2, hash(vec2(col, floor(uTime * 12.0))));
    vec3 c;

    if (uMode < 0.5) {
      // ---------------- STARS
      float zoom = 1.0 / (max(aspect, 1.0) * (1.035 + uLevel * 0.03));
      vec2 tc = s * zoom + 0.5;
      tc = flow(tc, 0.045 + uLevel * 0.06);
      tc = pixelate(tc);
      vec2 cen = vec2(0.5, 0.52);
      vec2 d = tc - cen;
      float r = length(s / vec2(max(aspect, 1.0) * 0.5, 0.5));
      float k = smoothstep(0.3, 1.25, r) * (0.2 + uLevel * 0.14);
      vec3 acc = vec3(0.0); float w = 0.0;
      for (int i = 0; i < 16; i++){
        float t = float(i) / 15.0;
        float wt = 1.0 - t * 0.55;
        vec2 p = cen + d * (1.0 - k * t);
        float ca = 0.006 * r;
        acc += vec3(tex(p + d * ca, 0.0).r, tex(p, 0.0).g, tex(p - d * ca, 0.0).b) * wt;
        w += wt;
      }
      c = acc / w;
      // horizontal smear lines near the sides (stretched pixels)
      float row = floor(uv.y * 240.0);
      float flick = hash(vec2(row, floor(uTime * 2.5)));
      float line = step(0.9, hash(vec2(row, 7.0))) * smoothstep(0.38, 1.0, abs(uv.x - 0.5) * 2.0) * (0.55 + 0.45 * flick);
      vec3 smear = tex(vec2(mix(tc.x, 0.5, 0.3), tc.y), 1.5) * 1.1;
      c = mix(c, smear, line * 0.45);
      c *= 0.8;
      c *= 1.0 - smoothstep(0.65, 1.55, r) * 0.6;
    } else {
      // ---------------- WORDS ("lens" scene from the reel)
      // the whole cover, enlarged, sharp around the subject (right of centre)
      // and melting into a heavy blur toward the left and the edges
      c = wordsBG(uv, s, aspect, 0.0, stripe);
      c *= 0.96;
    }

    // lens letters: words drawn into uMask show the cover's own pixels,
    // magnified and stretched vertically (the orange streaks in the reel)
    vec4 m = texture(uMask, uv);
    float lw = m.r * m.a;
    if (lw > 0.002) {
      float yc = m.g, wx = m.b;                        // centre of that word
      // look into the sharp subject of the cover, stretched tall into streaks
      vec2 subj = uMode < 0.5 ? vec2(0.5, 0.5) : vec2(0.63, 0.47);
      float wob = sin(uv.x * 90.0 + uTime * 3.0) * 0.004;
      vec2 luv = vec2(subj.x + (uv.x - wx) * 0.55, subj.y + (uv.y - yc) * 0.08 + wob);
      vec2 ls = (luv - 0.5) * vec2(aspect, 1.0);
      vec3 lc = uMode < 0.5 ? tex(ls * (1.0 / max(aspect, 1.0)) + 0.5, 0.0) : wordsBG(luv, ls, aspect, 1.0, 0.0);
      float ll = dot(lc, vec3(0.299, 0.587, 0.114));
      lc = mix(vec3(ll), lc, 1.6) * 1.4 + vec3(0.07, 0.035, 0.0);
      c = mix(c, lc, clamp(lw, 0.0, 1.0));
    }
    c += (hash(uv * uRes + fract(uTime)) - 0.5) * 0.018; // film grain
    o = vec4(c, 1.0);
  }`;

  function sh(type, src) {
    const s = gl.createShader(type);
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
  for (const n of ['uTex', 'uRes', 'uImgAspect', 'uTime', 'uLevel', 'uMode', 'uPix', 'uGlitch', 'uSeed', 'uMask']) U[n] = gl.getUniformLocation(prog, n);

  // lens mask: R = lens strength, G = word's vertical centre, A = glyph coverage
  const maskCanvas = document.createElement('canvas');
  const maskCtx = maskCanvas.getContext('2d');
  const maskTex = gl.createTexture();
  let maskDirty = true, maskEmpty = false;
  function uploadMask() {
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, maskTex);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, maskCanvas);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.activeTexture(gl.TEXTURE0);
    maskDirty = false;
  }
  // items: [{ kind: 'text'|'box', x, y, w, h, text, font, rot, strength }] in CSS px
  function setLens(items) {
    if (!items.length && maskEmpty) return;
    const k = maskCanvas.width / innerWidth;
    maskCtx.setTransform(1, 0, 0, 1, 0, 0);
    maskCtx.clearRect(0, 0, maskCanvas.width, maskCanvas.height);
    for (const it of items) {
      const g = Math.round(255 * (1 - it.y / innerHeight));     // flipped to GL's uv
      const bx = Math.round(255 * Math.min(1, Math.max(0, it.x / innerWidth)));
      maskCtx.fillStyle = `rgb(${Math.round(255 * it.strength)}, ${g}, ${bx})`;
      maskCtx.setTransform(k, 0, 0, k, it.x * k, it.y * k);
      maskCtx.rotate((it.rot || 0) * Math.PI / 180);
      if (it.kind === 'box') {
        maskCtx.beginPath();
        maskCtx.roundRect(-it.w / 2, -it.h / 2, it.w, it.h, it.h * 0.15);
        maskCtx.fill();
      } else {
        maskCtx.font = it.font;
        maskCtx.textAlign = 'center';
        maskCtx.textBaseline = 'middle';
        maskCtx.fillText(it.text, 0, 0);
      }
    }
    maskEmpty = !items.length;
    maskDirty = true;
  }

  const texture = gl.createTexture();
  const aniso = gl.getExtension('EXT_texture_filter_anisotropic');
  let hasTex = false, imgAspect = 1;

  function setImage(img) {
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    if (aniso) gl.texParameterf(gl.TEXTURE_2D, aniso.TEXTURE_MAX_ANISOTROPY_EXT, Math.min(8, gl.getParameter(aniso.MAX_TEXTURE_MAX_ANISOTROPY_EXT)));
    imgAspect = (img.naturalWidth || img.width) / (img.naturalHeight || img.height) || 1;
    hasTex = true;
  }

  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    let w = innerWidth * dpr, h = innerHeight * dpr;
    const max = 6.0e6; // keep 4K screens smooth
    if (w * h > max) { const k = Math.sqrt(max / (w * h)); w *= k; h *= k; }
    canvas.width = Math.max(2, w | 0);
    canvas.height = Math.max(2, h | 0);
    gl.viewport(0, 0, canvas.width, canvas.height);
    maskCanvas.width = Math.max(2, canvas.width >> 1);
    maskCanvas.height = Math.max(2, canvas.height >> 1);
    maskEmpty = false;
    setLens([]);
  }

  function draw(u) {
    if (!hasTex) return;
    gl.useProgram(prog);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.uniform1i(U.uTex, 0);
    gl.uniform2f(U.uRes, canvas.width, canvas.height);
    gl.uniform1f(U.uImgAspect, imgAspect);
    gl.uniform1f(U.uTime, u.time);
    gl.uniform1f(U.uLevel, u.level);
    gl.uniform1f(U.uMode, u.mode);
    gl.uniform1f(U.uPix, u.pix);
    gl.uniform1f(U.uGlitch, u.glitch);
    gl.uniform1f(U.uSeed, u.seed || 0);
    if (maskDirty) uploadMask();
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, maskTex);
    gl.uniform1i(U.uMask, 1);
    gl.activeTexture(gl.TEXTURE0);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  resize();
  return { setImage, resize, draw, setLens };
})();
