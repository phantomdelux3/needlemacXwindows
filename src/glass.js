// Liquid glass: SVG displacement-map refraction (lens-like bending at the
// rounded edges + slight RGB dispersion + soft blur). Works with both
// `filter:` (island, over a desktop snapshot) and `backdrop-filter:` (full view).
window.LiquidGlass = (() => {
  const NS = 'http://www.w3.org/2000/svg';
  let svg = null;
  const filters = {};

  function root() {
    if (svg) return svg;
    svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('width', '0');
    svg.setAttribute('height', '0');
    svg.style.cssText = 'position:absolute;width:0;height:0;overflow:hidden;pointer-events:none';
    svg.appendChild(document.createElementNS(NS, 'defs'));
    document.body.appendChild(svg);
    return svg;
  }

  // R/G encode the x/y displacement (128 = none). Inside the bezel the sample
  // point is pulled inwards, strongest at the rim – like the edge of a lens.
  function dispMap(w, h, r, bezel) {
    w = Math.max(4, Math.round(w));
    h = Math.max(4, Math.round(h));
    r = Math.min(r, w / 2, h / 2);
    const c = document.createElement('canvas');
    c.width = w; c.height = h;
    const ctx = c.getContext('2d');
    const im = ctx.createImageData(w, h);
    const d = im.data;
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const px = x + 0.5 - w / 2, py = y + 0.5 - h / 2;
        const qx = Math.abs(px) - (w / 2 - r), qy = Math.abs(py) - (h / 2 - r);
        let nx = 0, ny = 0, dist;
        if (qx > 0 && qy > 0) {
          const l = Math.hypot(qx, qy) || 1;
          dist = r - l; nx = (qx / l) * Math.sign(px); ny = (qy / l) * Math.sign(py);
        } else if (qx > qy) {
          dist = r - qx; nx = Math.sign(px);
        } else {
          dist = r - qy; ny = Math.sign(py);
        }
        let m = 0;
        if (dist >= 0 && dist < bezel) {
          const t = 1 - dist / bezel;
          m = Math.pow(t, 2.1);
        }
        const i = (y * w + x) * 4;
        d[i] = 128 - nx * m * 127;
        d[i + 1] = 128 - ny * m * 127;
        d[i + 2] = 128;
        d[i + 3] = 255;
      }
    }
    ctx.putImageData(im, 0, 0);
    return c.toDataURL();
  }

  function el(tag, attrs) {
    const e = document.createElementNS(NS, tag);
    for (const k in attrs) e.setAttribute(k, attrs[k]);
    return e;
  }

  // Creates (once) a filter `id`. opts: { scale, blur, dispersion }
  function ensure(id, opts = {}) {
    if (filters[id]) return filters[id];
    const scale = opts.scale ?? 60, blur = opts.blur ?? 2, disp = opts.dispersion ?? 0.08;
    const f = el('filter', {
      id, x: '0', y: '0', width: '100%', height: '100%',
      filterUnits: 'objectBoundingBox', primitiveUnits: 'userSpaceOnUse',
      'color-interpolation-filters': 'sRGB',
    });
    const img = el('feImage', { x: '0', y: '0', width: '100', height: '100', preserveAspectRatio: 'none', result: 'map' });
    f.appendChild(img);
    const ch = [
      ['R', scale, '1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0'],
      ['G', scale * (1 - disp), '0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0'],
      ['B', scale * (1 - disp * 2), '0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0'],
    ];
    const maps = [];
    for (const [c, s, m] of ch) {
      const dm = el('feDisplacementMap', { in: 'SourceGraphic', in2: 'map', scale: String(s), xChannelSelector: 'R', yChannelSelector: 'G', result: 'd' + c });
      maps.push(dm);
      f.appendChild(dm);
      f.appendChild(el('feColorMatrix', { in: 'd' + c, type: 'matrix', values: m, result: 'c' + c }));
    }
    f.appendChild(el('feBlend', { in: 'cR', in2: 'cG', mode: 'screen', result: 'rg' }));
    f.appendChild(el('feBlend', { in: 'rg', in2: 'cB', mode: 'screen', result: 'rgb' }));
    f.appendChild(el('feGaussianBlur', { in: 'rgb', stdDeviation: String(blur) }));
    root().firstChild.appendChild(f);
    filters[id] = { f, img, maps, key: '', scale, disp };
    return filters[id];
  }

  // Fit the refraction map to an element size / corner radius.
  function size(id, w, h, r, bezel) {
    const F = filters[id];
    if (!F) return;
    const key = `${Math.round(w)}x${Math.round(h)}r${Math.round(r)}b${Math.round(bezel)}`;
    if (F.key === key) return;
    F.key = key;
    F.img.setAttribute('href', dispMap(w, h, r, bezel));
    F.img.setAttribute('width', String(Math.round(w)));
    F.img.setAttribute('height', String(Math.round(h)));
  }

  // Stretch the existing map while an element animates between sizes.
  function stretch(id, w, h) {
    const F = filters[id];
    if (!F) return;
    F.img.setAttribute('width', String(Math.round(w)));
    F.img.setAttribute('height', String(Math.round(h)));
  }

  return { ensure, size, stretch, dispMap };
})();
