# `alice.display.set_backlight(name, value)`

Устанавливает яркость display backlight.

- **Permission:** `display`
- **Аргументы:** идентификатор `name` и целое значение `value`; диапазон
  определяется конкретным backend.
- **Результат:** `nil` при успехе.
- **Доступность Mini:** нет (`display.backlight=false`).

```lua
alice.display.set_backlight("display", 50)
```
