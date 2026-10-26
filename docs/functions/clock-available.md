# `alice.clock.available()`

Возвращает boolean, сообщает ли профиль устройства capability `clock`.

- **Permission:** `clock`
- **Аргументы:** нет.
- **Результат:** `true` или `false`; не вызывает устройство.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

Также доступен эквивалентный predicate `alice.clock.is_available()`.