# `alice.sensors.list()`

Возвращает найденные датчики и их текущие значения.

- **Permission:** `sensors`
- **Аргументы:** нет.
- **Результат:** массив таблиц `{id, kind, unit, value}`.

```lua
for _, sensor in ipairs(alice.sensors.list()) do
    print(sensor.id, sensor.kind, sensor.value, sensor.unit)
end
```

Единица измерения и диапазон значения зависят от типа сенсора и указаны в
полях `unit` и `kind`.