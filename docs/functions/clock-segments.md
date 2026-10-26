# `alice.clock.segments(levels)`

Задаёт уровни сегментов часов. Метод сохраняет callable-форму YAM API.

- **Permission:** `clock`
- **Аргумент:** таблица `имя сегмента -> целое значение 0..255`.
- **Результат:** `nil` при успехе.

```lua
alice.clock.segments({ hour_1 = 255, minute_1 = 128 })
```

Для проверки используйте `alice.clock.segments.is_available()`.