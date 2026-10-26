# `alice.bluetooth.disconnect(address)`

Запрашивает отключение Bluetooth-устройства.

- **Permission:** `bluetooth`
- **Аргумент:** MAC-адрес; формат проверяется до вызова backend.
- **Доступность Mini:** нет (`bluetooth.disconnect=false`); операция не
  реализована.
- **Результат:** на поддерживающем backend — `nil` либо ошибка API.

```lua
alice.bluetooth.disconnect("01:23:45:67:89:ab")
```
