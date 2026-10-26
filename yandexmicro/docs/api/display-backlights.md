# `alice.display.backlights()`

Возвращает перечень доступных display backlight identifiers.

- **Permission:** `display`
- **Аргументы:** нет.
- **Результат:** таблица объектов, формат задаёт Device API backend.
- **Доступность Mini:** нет (`display.backlights=false`).

```lua
local lights = alice.display.backlights()
```

Вызов на Mini завершится понятной ошибкой неподдерживаемой операции.
