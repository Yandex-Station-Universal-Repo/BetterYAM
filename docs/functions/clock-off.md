# `alice.clock.off()`

Выключает поддерживаемые сегменты часов.

- **Permission:** `clock`
- **Аргументы:** нет.
- **Результат:** `nil` при успехе.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
alice.clock.off()
```