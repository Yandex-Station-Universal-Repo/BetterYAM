# `alice.bluetooth.remove(address)`

Запрашивает удаление Bluetooth-устройства из списка сопряжённых.

- **Permission:** `bluetooth`
- **Аргумент:** MAC-адрес; формат проверяется до вызова backend.
- **Результат:** на поддерживающем backend — `nil` либо ошибка API.

```lua
alice.bluetooth.remove("01:23:45:67:89:ab")
```