# `alice.off()`

Выключает LED ring.

- **Permission:** `led`
- **Аргументы:** нет.
- **Результат:** `nil` при успехе; ошибка backend передаётся Lua.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
alice.off()
```