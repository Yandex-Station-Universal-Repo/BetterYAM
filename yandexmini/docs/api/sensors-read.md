# `alice.sensors.read(id)`

Читает текущее значение сенсора с заданным идентификатором.

- **Permission:** `sensors`
- **Аргумент:** `id` из результата [`alice.sensors.list`](sensors-list.md).
- **Результат:** `{id, kind, unit, value}`; для отсутствующего сенсора
  возвращается ошибка.
- **Доступность Mini:** sensor `stm-vl53l0-proximity-sensor`, тип `proximity`,
  единица `mm`.

```lua
local reading = alice.sensors.read("stm-vl53l0-proximity-sensor")
```
