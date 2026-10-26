# `alice.bluetooth.disconnect(address)`

Запрашивает отключение Bluetooth-устройства.

- **Permission:** `bluetooth`
- **Аргумент:** MAC-адрес; формат проверяется до вызова backend.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.
- **Результат:** на поддерживающем backend — `nil` либо ошибка API.

```lua
alice.bluetooth.disconnect("01:23:45:67:89:ab")
```