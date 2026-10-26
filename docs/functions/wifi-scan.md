# `alice.wifi.scan()`

Запускает поиск Wi-Fi сетей поблизости.

- **Permission:** `wifi`
- **Аргументы:** нет.
- **Результат:** boolean; `false`, если сканирование недоступно в текущем
  режиме точки доступа.

```lua
local started = alice.wifi.scan()
```