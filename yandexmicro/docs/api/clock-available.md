# `alice.clock.available()`

Возвращает boolean, сообщает ли профиль устройства capability `clock`.

- **Permission:** `clock`
- **Аргументы:** нет.
- **Результат:** `true` или `false`; не вызывает устройство.
- **Доступность Mini:** метод доступен, но на проверенном профиле возвращает
  `false`.

Также доступен эквивалентный predicate `alice.clock.is_available()`.
