# `alice.ipc.alarm.is_available()`

Проверяет доступность alarm IPC.

- **Permission:** `ipc.alarm`
- **Результат:** boolean без ожидания или чтения события.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.