# `alice.display.set_backlight(name, value)`

Устанавливает яркость display backlight.

- **Permission:** `display`
- **Аргументы:** идентификатор `name` и целое значение `value`; диапазон
  определяется конкретным backend.
- **Результат:** `nil` при успехе.

```lua
alice.display.set_backlight("display", 50)
```