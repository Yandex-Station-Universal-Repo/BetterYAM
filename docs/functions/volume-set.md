# `alice.volume.set(percent)`

Устанавливает уровень громкости.

- **Permission:** `volume`
- **Аргумент:** целое число `0..100`.
- **Результат:** `nil` при успехе; ошибка ALSA передаётся Lua.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
alice.volume.set(35)
```