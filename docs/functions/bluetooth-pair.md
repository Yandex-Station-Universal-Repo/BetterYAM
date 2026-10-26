# `alice.bluetooth.pair(address)`

Запрашивает сопряжение с Bluetooth-устройством.

- **Permission:** `bluetooth`
- **Аргумент:** MAC-адрес; формат проверяется до вызова backend.
- **Результат:** на поддерживающем backend — `nil` либо ошибка API.

```lua
alice.bluetooth.pair("01:23:45:67:89:ab")
```