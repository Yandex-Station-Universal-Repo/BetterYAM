# `alice.volume.get()`

Читает текущую громкость.

- **Permission:** `volume`
- **Аргументы:** нет.
- **Результат:** целое значение процента `0..100`.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
local percent = alice.volume.get()
```