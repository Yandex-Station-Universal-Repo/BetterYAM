# `alice.logo.set(colors)`

Совместимый alias `alice.set(colors)` для логотипа/LED ring.

- **Permission:** `led`
- **Аргумент:** последовательность цветов в том же формате, что и
  [`alice.set`](led-set.md).
- **Результат:** `nil` при успехе.

```lua
local colors = {}
for i = 1, device.led_count do
    colors[i] = "#ffffff"
end
alice.logo.set(colors)
```