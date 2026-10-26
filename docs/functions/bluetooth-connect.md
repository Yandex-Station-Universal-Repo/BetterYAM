# `alice.bluetooth.connect(address)`

Запрашивает подключение к ранее известному Bluetooth-устройству.

- **Permission:** `bluetooth`
- **Аргумент:** MAC-адрес; формат проверяется до вызова backend.
- **Результат:** на поддерживающем backend — `nil` либо ошибка API.

```lua
alice.bluetooth.connect("01:23:45:67:89:ab")
```