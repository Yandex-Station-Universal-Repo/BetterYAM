# `alice.bluetooth.power(on)`

Запрашивает включение или выключение Bluetooth.

- **Permission:** `bluetooth`
- **Аргумент:** boolean.
- **Результат:** `nil` при успехе; системная ошибка или причина отказа
  передаётся Lua.

```lua
alice.bluetooth.power(true)
```