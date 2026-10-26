# `alice.led.is_available()`

Проверяет capability `led` в активном device profile.

- **Permission:** `led`
- **Результат:** boolean; метод не отправляет frame.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
if alice.led.is_available() then alice.off() end
```