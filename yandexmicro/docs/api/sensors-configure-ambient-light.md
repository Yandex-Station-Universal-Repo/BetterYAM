# `alice.sensors.configure_ambient_light(name, gain, integration_ms)`

Настраивает ambient-light sensor.

- **Permission:** `sensors.control` (а также доступ к таблице sensors).
- **Аргументы:** имя сенсора; `gain` из `1, 3, 6, 9, 18`; время интеграции из
  `25, 50, 100, 200, 400` ms.
- **Результат:** `nil` при успехе.
- **Доступность Mini:** нет (`sensors.ambient_light.configure=false`).

```lua
alice.sensors.configure_ambient_light("ambient", 3, 100)
```
