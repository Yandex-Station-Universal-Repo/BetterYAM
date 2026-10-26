# `alice.input.is_available()`

Проверяет общую capability чтения input events.

- **Permission:** `input`
- **Результат:** boolean без чтения очереди событий.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.