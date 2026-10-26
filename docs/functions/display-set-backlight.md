# `alice.display.set_backlight(name, value)`

Устанавливает яркость display backlight.

- **Permission:** `display`
- **Аргументы:** идентификатор `name` и целое значение `value`; диапазон
  определяется конкретным backend.
- **Результат:** `nil` при успехе.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
alice.display.set_backlight("display", 50)
```