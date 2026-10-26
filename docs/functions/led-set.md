# `alice.set(colors)`

Устанавливает цвета LED ring, сохраняя совместимый YAM API.

- **Permission:** `led`
- **Аргумент:** последовательность Lua-строк цветов, обычно `"#RRGGBB"`.
- **Результат:** `nil` при успехе; ошибка backend передаётся Lua.

```lua
local colors = {}
for i = 1, device.led_count do
    colors[i] = "#000000"
end
colors[1] = "#ff0000"
alice.set(colors)
```

См. также [`alice.off`](led-off.md), [`alice.play`](led-play.md).