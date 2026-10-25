# `alice.bluetooth.scan(on)`

Запрашивает запуск или остановку поиска Bluetooth-устройств.

- **Permission:** `bluetooth`
- **Аргумент:** boolean.
- **Результат:** на поддерживающем backend — результат стандартного Device API.
- **Доступность Mini:** нет (`bluetooth.scan=false`); метод сохраняется для
  совместимости, вызов Mini завершится ошибкой «operation not supported».

```lua
alice.bluetooth.scan(true)
```

Discovery вручную запускали через интерактивный vendor `bt_demo`, но эта
проверка не относится к `alice.bluetooth.scan`: BetterYAM не управляет
`bt_demo` и не объявляет scan capability. Утилита может перезапускать
`bsa_server`; её нельзя использовать как надёжную замену API daemon.
