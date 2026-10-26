# `alice.sensors.is_available()`

Проверяет доступность хотя бы одной сенсорной операции.

- **Permission:** `sensors`
- **Результат:** boolean; датчик не считывается этим predicate.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.