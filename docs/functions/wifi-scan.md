# `alice.wifi.scan()`

Запускает поиск Wi-Fi сетей поблизости.

- **Permission:** `wifi`
- **Аргументы:** нет.
- **Результат:** boolean; `false`, если сканирование недоступно в текущем
  режиме точки доступа.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
local started = alice.wifi.scan()
```