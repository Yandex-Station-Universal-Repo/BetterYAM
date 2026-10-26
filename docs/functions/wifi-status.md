# `alice.wifi.status()`

Возвращает состояние Wi-Fi: текущую сеть, сохранённые/обнаруженные сети и
состояние setup access point. Пароли не возвращаются.

- **Permission:** `wifi`
- **Аргументы:** нет.
- **Результат:** таблица статуса.

```lua
local status = alice.wifi.status()
print(status.current and status.current.ssid)
```