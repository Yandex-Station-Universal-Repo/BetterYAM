# `alice.wifi.status()`

Возвращает состояние Wi-Fi: текущую сеть, сохранённые/обнаруженные сети и
состояние setup access point. Пароли не возвращаются.

- **Permission:** `wifi`
- **Аргументы:** нет.
- **Результат:** таблица статуса.
- **Доступность Mini:** да, если обнаружены `wpa_cli` и интерфейс `wlan0`.

```lua
local status = alice.wifi.status()
print(status.current and status.current.ssid)
```
