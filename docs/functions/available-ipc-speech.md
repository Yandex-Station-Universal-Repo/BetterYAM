# `alice.ipc.speech.is_available()`

Проверяет доступность speech IPC.

- **Permission:** `ipc.speech`
- **Результат:** boolean без ожидания или чтения речи.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.