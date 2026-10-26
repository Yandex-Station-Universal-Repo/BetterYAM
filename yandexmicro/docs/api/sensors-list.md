# `alice.sensors.list()`

Возвращает найденные датчики и их текущие значения.

- **Permission:** `sensors`
- **Аргументы:** нет.
- **Результат:** массив таблиц `{id, kind, unit, value}`.
- **Доступность Mini:** proximity sensor доступен через Linux input
  `ABS_DISTANCE`.

```lua
for _, sensor in ipairs(alice.sensors.list()) do
    print(sensor.id, sensor.kind, sensor.value, sensor.unit)
end
```

Для обнаруженного сенсора Mini значение возвращается в миллиметрах; за
допустимым диапазоном `0..8192` возвращается `nil`.
