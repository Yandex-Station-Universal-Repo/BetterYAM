# `alice.display.backlights()`

Возвращает перечень доступных display backlight identifiers.

- **Permission:** `display`
- **Аргументы:** нет.
- **Результат:** таблица объектов, формат задаёт Device API backend.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
local lights = alice.display.backlights()
```
