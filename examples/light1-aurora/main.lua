local function rgb(r, g, b)
  return string.format("#%02X%02X%02X", r, g, b)
end

local function wave(x)
  return math.floor((math.sin(x) + 1) * 127.5)
end

for frame = 0, 719 do
  local pixels = {}
  local t = frame / 12
  for i = 1, device.led_count do
    local p = t + (i - 1) * 0.72
    local r = math.floor(25 + wave(p + 2.1) * 0.35)
    local g = math.floor(45 + wave(p + 0.7) * 0.55)
    local b = math.floor(95 + wave(p) * 0.63)
    pixels[i] = rgb(r, g, b)
  end
  alice.set(pixels)
  alice.sleep(60)
end
