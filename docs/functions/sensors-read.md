# `alice.sensors.read(id)`

Читает текущее значение сенсора с заданным идентификатором.

- **Permission:** `sensors`
- **Аргумент:** `id` из результата [`alice.sensors.list`](sensors-list.md).
- **Результат:** `{id, kind, unit, value}`; для отсутствующего сенсора
  возвращается ошибка.
- **Доступность:** проверяйте capability этой операции в `device.capabilities`;
  отсутствие capability означает, что операция недоступна.

```lua
local sensors = alice.sensors.list()
local sensor = sensors[1]
if sensor then
    local reading = alice.sensors.read(sensor.id)
    print(reading.value, reading.unit)
end
```
