local n = device.led_count

local function hsv(h)
  local i = math.floor(h * 6) % 6
  local f = h * 6 - math.floor(h * 6)
  local q, t = 1 - f, f
  local r, g, b = table.unpack(({
    {1, t, 0}, {q, 1, 0}, {0, 1, t}, {0, q, 1}, {t, 0, 1}, {1, 0, q},
  })[i + 1])
  local function byte(x)
    return math.floor(x * 255 + 0.5)
  end
  return string.format("#%02X%02X%02X", byte(r), byte(g), byte(b))
end

for frame = 0, 47 do
  local colors = {}
  for i = 0, n - 1 do
    colors[i + 1] = hsv(((i / n) + frame / 24) % 1)
  end
  alice.set(colors)
  alice.sleep(40)
end
alice.off()

alice.clock.time("12:34", 255)
alice.sleep(3000)
alice.clock.off()
