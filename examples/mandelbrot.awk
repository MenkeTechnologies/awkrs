# Mandelbrot set ASCII render — escape-time over a rectangular window.
# Input first line: "W <w> H <h> ITER <n>"   render width / height / max iterations
# Output: h rows of width-w cells, each char from the shading ramp.
# Shading: " .:-=+*#%@" (lighter -> denser as escape time grows).
#
# Standard view: [-2.0, 1.0] x [-1.2, 1.2].

NR == 1 && $1 == "W" {
  W = $2 + 0; H = $4 + 0; ITER = $6 + 0
  xmin = -2.0; xmax = 1.0
  ymin = -1.2; ymax = 1.2
  ramp = " .:-=+*#%@"
  ramp_n = length(ramp)

  for (py = 0; py < H; py++) {
    cy = ymin + (ymax - ymin) * py / (H - 1)
    line = ""
    for (px = 0; px < W; px++) {
      cx = xmin + (xmax - xmin) * px / (W - 1)
      x = 0; y = 0; n = 0
      while (n < ITER && x * x + y * y <= 4) {
        x_new = x * x - y * y + cx
        y     = 2 * x * y + cy
        x     = x_new
        n++
      }
      if (n == ITER) idx = 1
      else {
        idx = int(n * (ramp_n - 1) / ITER) + 1
        if (idx > ramp_n) idx = ramp_n
        if (idx < 1) idx = 1
      }
      line = line substr(ramp, idx, 1)
    }
    print line
  }
  exit 0
}
