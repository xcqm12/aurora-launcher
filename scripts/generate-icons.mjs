import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const crcTable = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buffer) {
  let c = -1;
  for (let i = 0; i < buffer.length; i++) c = crcTable[(c ^ buffer[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const typeBuffer = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuffer, data])));
  return Buffer.concat([length, typeBuffer, data, crc]);
}

function encodePng(width, height, rgba) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 6;
  const rows = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    rows[y * (width * 4 + 1)] = 0;
    rgba.copy(rows, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", zlib.deflateSync(rows, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function mix(a, b, t) {
  return a + (b - a) * t;
}

function render(size) {
  const rgba = Buffer.alloc(size * size * 4);
  const s = size;
  for (let y = 0; y < s; y++) {
    for (let x = 0; x < s; x++) {
      const i = (y * s + x) * 4;
      const nx = (x + 0.5) / s;
      const ny = (y + 0.5) / s;
      const dx = nx - 0.5;
      const dy = ny - 0.52;

      let r = mix(9, 20, ny);
      let g = mix(14, 30, ny);
      let b = mix(26, 52, ny);
      let a = 255;

      const corner = 0.22;
      const qx = Math.max(Math.abs(nx - 0.5) - (0.5 - corner), 0);
      const qy = Math.max(Math.abs(ny - 0.5) - (0.5 - corner), 0);
      if (Math.hypot(qx, qy) > corner) a = 0;

      const glow = Math.max(0, 1 - Math.hypot(nx - 0.72, ny - 0.2) / 0.62);
      r = mix(r, 26, glow * 0.5);
      g = mix(g, 58, glow * 0.5);
      b = mix(b, 74, glow * 0.5);

      for (const [offset, strength, width] of [
        [0.32, 0.5, 0.035],
        [0.42, 0.34, 0.026],
      ]) {
        const wave = offset + 0.035 * Math.sin((nx - 0.5) * 6.2 + 0.4);
        const band = Math.abs(ny - wave);
        if (band < width) {
          const k = (1 - band / width) * strength;
          r = mix(r, 96, k);
          g = mix(g, 214, k);
          b = mix(b, 236, k);
        }
      }

      const radius = Math.hypot(dx, dy);
      const angle = Math.atan2(dy, dx);
      const profile = 0.34 + 0.66 * Math.pow(Math.abs(Math.cos(2 * angle)), 2.6);
      const starEdge = radius / (0.3 * profile);
      if (starEdge < 1) {
        const t = Math.pow(1 - starEdge, 0.55);
        r = mix(r, 150, t);
        g = mix(g, 247, t);
        b = mix(b, 206, t);
      } else if (starEdge < 1.13) {
        const halo = (1.13 - starEdge) / 0.13;
        r = mix(r, 120, halo * 0.35);
        g = mix(g, 240, halo * 0.35);
        b = mix(b, 200, halo * 0.35);
      }

      const vignette = Math.max(0, Math.hypot(dx, dy) - 0.42);
      const dim = Math.max(0.55, 1 - vignette * 1.1);
      r *= dim;
      g *= dim;
      b *= dim;

      rgba[i] = Math.round(Math.min(255, Math.max(0, r)));
      rgba[i + 1] = Math.round(Math.min(255, Math.max(0, g)));
      rgba[i + 2] = Math.round(Math.min(255, Math.max(0, b)));
      rgba[i + 3] = a;
    }
  }
  return encodePng(s, s, rgba);
}

function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  const entries = [];
  let offset = 6 + images.length * 16;
  for (const { size, data } of images) {
    const entry = Buffer.alloc(16);
    entry[0] = size >= 256 ? 0 : size;
    entry[1] = size >= 256 ? 0 : size;
    entry[2] = 0;
    entry[3] = 0;
    entry.writeUInt16LE(1, 4);
    entry.writeUInt16LE(32, 6);
    entry.writeUInt32LE(data.length, 8);
    entry.writeUInt32LE(offset, 12);
    entries.push(entry);
    offset += data.length;
  }
  return Buffer.concat([header, ...entries, ...images.map((image) => image.data)]);
}

const outDir = path.join(process.cwd(), "src-tauri", "icons");
fs.mkdirSync(outDir, { recursive: true });
const large = render(1024);
fs.writeFileSync(path.join(outDir, "icon.png"), large);
fs.writeFileSync(path.join(outDir, "32x32.png"), render(32));
fs.writeFileSync(path.join(outDir, "128x128.png"), render(128));
fs.writeFileSync(path.join(outDir, "128x128@2x.png"), render(256));
fs.writeFileSync(
  path.join(outDir, "icon.ico"),
  ico([16, 24, 32, 48, 64, 128, 256].map((size) => ({ size, data: render(size) }))),
);
console.log(`Generated Aurora icons in ${outDir}`);
