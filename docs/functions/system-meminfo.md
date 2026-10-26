# `alice.system.meminfo()`

Читает содержимое `/proc/meminfo`.

- **Permission:** `system`
- **Аргументы:** нет.
- **Результат:** строка в формате Linux `/proc/meminfo`.
- **Ошибка:** ошибка чтения файла передаётся Lua.