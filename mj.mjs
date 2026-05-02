// MathJax LaTeX→SVG converter. Reads one LaTeX string per line from stdin,
// writes one SVG per line to stdout. Sends "READY\n" on stderr when initialized.
import { createRequire } from 'module';
import { createInterface } from 'readline';
const require = createRequire(import.meta.url);

const MJ_PATH = process.env.MJ_PATH || 'C:/Users/Admin/AppData/Roaming/npm/node_modules/mathjax';
const MathJax = require(MJ_PATH);

// ex size in px: MathJax uses 0.5em for ex, default font-size 16px → 1ex = 8px
// But MathJax actually sizes based on surrounding text. We use 8px/ex here.
const EX_PX = 8;

const MJ = await MathJax.init({
  loader: { load: ['input/tex', 'output/svg'] },
  tex: { packages: { '[+]': ['ams'] } },
  svg: { fontCache: 'local', exFactor: 0.5 },
});

// Signal ready to parent process
process.stderr.write('READY\n');

function exToPx(val) {
  // val is like "11.78ex" or "4.104ex" — convert to px
  const m = String(val).match(/^([\d.]+)ex$/);
  if (m) return (parseFloat(m[1]) * EX_PX).toFixed(1) + 'px';
  return val;
}

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of rl) {
  if (!line.trim()) continue;
  try {
    const node = MJ.tex2svg(line, { display: true });
    let svg = MJ.startup.adaptor.innerHTML(node);
    // Replace ex-based width/height with px equivalents for resvg
    svg = svg.replace(/\bwidth="([\d.]+ex)"/g, (_, v) => `width="${exToPx(v)}"`);
    svg = svg.replace(/\bheight="([\d.]+ex)"/g, (_, v) => `height="${exToPx(v)}"`);
    process.stdout.write(svg.replace(/\n/g, ' ') + '\n');
  } catch (e) {
    process.stdout.write('ERROR:' + String(e).replace(/\n/g, ' ') + '\n');
  }
}
