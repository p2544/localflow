// Generates LocalFlow app icons (PNG, ICO, ICNS) with zero image deps:
// draws a rounded-square + waveform onto a raw RGBA buffer, encodes PNG via
// zlib, then wraps the PNGs in ICO/ICNS containers (both support embedded PNG).
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");
mkdirSync(OUT, { recursive: true });

function drawIcon(size) {
  const px = new Uint8Array(size * size * 4);
  const bg = [15, 23, 42, 255]; // slate-900
  const fg = [74, 222, 128, 255]; // green-400
  const r = size * 0.22; // corner radius
  const bars = [0.35, 0.65, 1.0, 0.55, 0.8, 0.4, 0.7]; // waveform heights
  const barW = size / (bars.length * 2 + 1);

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      // rounded-rect mask
      const dx = Math.max(r - x, x - (size - 1 - r), 0);
      const dy = Math.max(r - y, y - (size - 1 - r), 0);
      if (dx * dx + dy * dy > r * r) continue; // transparent corner
      let c = bg;
      // waveform bars, vertically centered
      const bi = Math.floor(x / (barW * 2) - 0.5);
      const inBarX = x % (barW * 2) >= barW * 0.75 && x % (barW * 2) < barW * 1.75;
      if (bi >= 0 && bi < bars.length && inBarX) {
        const h = bars[bi] * size * 0.55;
        if (Math.abs(y - size / 2) < h / 2) c = fg;
      }
      px[i] = c[0]; px[i + 1] = c[1]; px[i + 2] = c[2]; px[i + 3] = c[3];
    }
  }
  return px;
}

function crc32(buf) {
  let c, table = crc32.table;
  if (!table) {
    table = crc32.table = new Int32Array(256).map((_, n) => {
      c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      return c;
    });
  }
  let crc = -1;
  for (const b of buf) crc = (crc >>> 8) ^ table[(crc ^ b) & 0xff];
  return (crc ^ -1) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function encodePng(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6; // 8-bit RGBA
  // raw scanlines with filter byte 0
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    Buffer.from(rgba.buffer, y * size * 4, size * 4).copy(raw, y * (size * 4 + 1) + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const png = (s) => encodePng(s, drawIcon(s));

// PNGs Tauri references directly
writeFileSync(join(OUT, "32x32.png"), png(32));
writeFileSync(join(OUT, "128x128.png"), png(128));
writeFileSync(join(OUT, "128x128@2x.png"), png(256));
writeFileSync(join(OUT, "icon.png"), png(512));

// ICO: single 256px PNG entry (Vista+ format)
const icoPng = png(256);
const icoHeader = Buffer.alloc(6 + 16);
icoHeader.writeUInt16LE(0, 0);   // reserved
icoHeader.writeUInt16LE(1, 2);   // type: icon
icoHeader.writeUInt16LE(1, 4);   // count
icoHeader[6] = 0;                // width 256 -> 0
icoHeader[7] = 0;                // height 256 -> 0
icoHeader.writeUInt16LE(1, 10);  // planes
icoHeader.writeUInt16LE(32, 12); // bpp
icoHeader.writeUInt32LE(icoPng.length, 14);
icoHeader.writeUInt32LE(22, 18); // offset
writeFileSync(join(OUT, "icon.ico"), Buffer.concat([icoHeader, icoPng]));

// ICNS: ic07 (128) + ic08 (256) + ic09 (512) PNG entries
function icnsEntry(type, data) {
  const h = Buffer.alloc(8);
  h.write(type, 0, "ascii");
  h.writeUInt32BE(data.length + 8, 4);
  return Buffer.concat([h, data]);
}
const entries = Buffer.concat([
  icnsEntry("ic07", png(128)),
  icnsEntry("ic08", png(256)),
  icnsEntry("ic09", png(512)),
]);
const icnsHeader = Buffer.alloc(8);
icnsHeader.write("icns", 0, "ascii");
icnsHeader.writeUInt32BE(entries.length + 8, 4);
writeFileSync(join(OUT, "icon.icns"), Buffer.concat([icnsHeader, entries]));

console.log("icons written to", OUT);
