# `alice.clock.is_available()`

Проверяет общую capability часов.

- **Permission:** `clock`
- **Результат:** boolean без команды устройству.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

Синоним `alice.clock.available()` документирован отдельно: [clock available](clock-available.md).