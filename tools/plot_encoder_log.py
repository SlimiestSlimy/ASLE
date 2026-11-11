#!/usr/bin/env python3
"""Charts an ASLE encoder log (JSON Lines) as one self-contained HTML page.

Small multiples over every committed change, sharing the x axis (commit number):
leaves, encoded symbol bits, header bits (measured only), total bits.
Window-size boundaries are marked; merges with deletion are marked on each line.

usage: python3 tools/plot_encoder_log.py <encoder_log.jsonl> <chart.html>
"""
import json
import sys


def read_commit_points(log_path):
    commit_points = []
    for line in open(log_path):
        event = json.loads(line)
        if event["event"] == "build_started":
            commit_points.append({
                "commit_number": 0, "window_bit_length": 0, "kind": "start", "symbol": "",
                "leaves": event["leaves"], "encoded_symbol_bits": event["encoded_symbol_bits"],
                "header_bits": event["header_bits"], "encoded_symbol_bits_plus_header_bits": event["encoded_symbol_bits_plus_header_bits"],
            })
        elif event["event"] == "change_committed":
            commit_points.append({key: event[key] for key in (
                "commit_number", "window_bit_length", "kind", "symbol", "leaves",
                "encoded_symbol_bits", "header_bits", "encoded_symbol_bits_plus_header_bits")})
    return commit_points


PAGE_TEMPLATE = r"""<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>ASLE encoder log: __TITLE__</title>
<style>
.viz-root { color-scheme: light; --surface-1:#fcfcfb; --text-primary:#0b0b0b; --text-secondary:#52514e;
  --grid:#e4e3df; --series-1:#2a78d6; --marker:#eb6834; --boundary:#c9c8c3;
  font: 13px/1.4 system-ui, sans-serif; background: var(--surface-1); color: var(--text-primary); padding: 16px; }
@media (prefers-color-scheme: dark) { .viz-root { color-scheme: dark; --surface-1:#1a1a19; --text-primary:#f0efec;
  --text-secondary:#c3c2b7; --grid:#33332f; --series-1:#3987e5; --marker:#d95926; --boundary:#4a4a45; } }
h1 { font-size: 16px; margin: 0 0 4px; } p { margin: 0 0 12px; color: var(--text-secondary); }
.panel-title { font-weight: 600; margin: 14px 0 2px; } svg { display: block; width: 100%; height: auto; }
.tooltip { position: fixed; pointer-events: none; background: var(--surface-1); color: var(--text-primary);
  border: 1px solid var(--grid); border-radius: 6px; padding: 6px 8px; font-size: 12px; display: none;
  box-shadow: 0 2px 8px rgba(0,0,0,.15); white-space: nowrap; }
.key-swatch { display:inline-block; width:8px; height:8px; border-radius:50%; background:var(--marker); margin-right:4px; }
</style></head>
<body class="viz-root">
<h1>ASLE encoder log: __TITLE__</h1>
<p>Every committed change, in order. Vertical lines mark new window sizes.
<span class="key-swatch"></span>marks a merge with deletion. Header bits are measured only, never used to decide.</p>
<div id="panels"></div><div class="tooltip" id="tooltip"></div>
<script>
const points = __DATA__;
const panels = [
  ["leaves", "Leaves (dictionary size)"],
  ["encoded_symbol_bits", "Encoded symbol bits"],
  ["header_bits", "Header bits (measured only)"],
  ["encoded_symbol_bits_plus_header_bits", "Total bits (encoded symbols + header)"],
];
const width = 960, height = 170, marginLeft = 64, marginRight = 12, marginTop = 10, marginBottom = 22;
const lastCommit = points[points.length - 1].commit_number || 1;
const xOf = c => marginLeft + (c / lastCommit) * (width - marginLeft - marginRight);
const windowStarts = points.filter((p, i) => i > 0 && p.window_bit_length !== points[i - 1].window_bit_length);
const container = document.getElementById("panels"), tooltip = document.getElementById("tooltip");
const ns = "http://www.w3.org/2000/svg";
function element(name, attributes, parent) {
  const e = document.createElementNS(ns, name);
  for (const k in attributes) e.setAttribute(k, attributes[k]);
  parent.appendChild(e); return e;
}
const crosshairs = [];
for (const [key, title] of panels) {
  const heading = document.createElement("div"); heading.className = "panel-title"; heading.textContent = title;
  container.appendChild(heading);
  const svg = element("svg", { viewBox: `0 0 ${width} ${height}`, role: "img", "aria-label": title }, container);
  const values = points.map(p => p[key]);
  const low = Math.min(...values), high = Math.max(...values), span = (high - low) || 1;
  const yOf = v => marginTop + (1 - (v - low) / span) * (height - marginTop - marginBottom);
  for (let t = 0; t <= 2; t++) {
    const v = low + span * t / 2, y = yOf(v);
    element("line", { x1: marginLeft, x2: width - marginRight, y1: y, y2: y, stroke: "var(--grid)", "stroke-width": 1 }, svg);
    element("text", { x: marginLeft - 6, y: y + 4, "text-anchor": "end", fill: "var(--text-secondary)", "font-size": 11 }, svg)
      .textContent = Math.round(v).toLocaleString();
  }
  let lastLabelX = -Infinity;
  const minimumLabelSpacing = 30; // skip a window-size label that would overlap the previous one
  for (const p of windowStarts) {
    const x = xOf(p.commit_number - 0.5);
    element("line", { x1: x, x2: x, y1: marginTop, y2: height - marginBottom, stroke: "var(--boundary)", "stroke-width": 1 }, svg);
    if (x - lastLabelX >= minimumLabelSpacing) {
      element("text", { x: x + 2, y: height - 6, fill: "var(--text-secondary)", "font-size": 10 }, svg).textContent = "L=" + p.window_bit_length;
      lastLabelX = x;
    }
  }
  element("polyline", { points: points.map(p => `${xOf(p.commit_number)},${yOf(p[key])}`).join(" "),
    fill: "none", stroke: "var(--series-1)", "stroke-width": 2, "stroke-linejoin": "round" }, svg);
  for (const p of points.filter(p => p.kind === "merge_with_deletion"))
    element("circle", { cx: xOf(p.commit_number), cy: yOf(p[key]), r: 3.5, fill: "var(--marker)",
      stroke: "var(--surface-1)", "stroke-width": 2 }, svg);
  const crosshair = element("line", { y1: marginTop, y2: height - marginBottom, stroke: "var(--text-secondary)",
    "stroke-width": 1, visibility: "hidden" }, svg);
  crosshairs.push(crosshair);
  svg.addEventListener("mousemove", event => {
    const box = svg.getBoundingClientRect();
    const svgX = (event.clientX - box.left) / box.width * width;
    const commit = Math.round((svgX - marginLeft) / (width - marginLeft - marginRight) * lastCommit);
    const p = points[Math.max(0, Math.min(points.length - 1, commit))];
    for (const c of crosshairs) { c.setAttribute("x1", xOf(p.commit_number)); c.setAttribute("x2", xOf(p.commit_number)); c.setAttribute("visibility", "visible"); }
    tooltip.style.display = "block";
    tooltip.style.left = (event.clientX + 14) + "px"; tooltip.style.top = (event.clientY + 14) + "px";
    tooltip.innerHTML = `commit ${p.commit_number} · window ${p.window_bit_length} · ${p.kind}<br>` +
      (p.symbol ? `symbol ${p.symbol}<br>` : "") +
      `leaves ${p.leaves.toLocaleString()} · encoded symbol bits ${p.encoded_symbol_bits.toLocaleString()}<br>` +
      `header ${p.header_bits.toLocaleString()} · total ${p.encoded_symbol_bits_plus_header_bits.toLocaleString()}`;
  });
  svg.addEventListener("mouseleave", () => { tooltip.style.display = "none"; for (const c of crosshairs) c.setAttribute("visibility", "hidden"); });
}
</script></body></html>
"""


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    log_path, chart_path = sys.argv[1], sys.argv[2]
    commit_points = read_commit_points(log_path)
    title = log_path.rsplit("/", 1)[-1]
    page = PAGE_TEMPLATE.replace("__TITLE__", title).replace("__DATA__", json.dumps(commit_points))
    open(chart_path, "w").write(page)
    print(f"{chart_path}: {len(commit_points) - 1} commits charted")


if __name__ == "__main__":
    main()
