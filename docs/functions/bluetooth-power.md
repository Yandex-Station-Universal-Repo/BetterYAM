# `alice.bluetooth.power(on)`

Запрашивает включение или выключение Bluetooth.

- **Permission:** `bluetooth`
- **Аргумент:** boolean.
- **Результат:** `nil` при успехе; системная ошибка или причина отказа
  передаётся Lua.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
alice.bluetooth.power(true)
```