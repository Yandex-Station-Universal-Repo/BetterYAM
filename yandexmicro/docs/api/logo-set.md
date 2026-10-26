# `alice.logo.set(colors)`

Совместимый alias `alice.set(colors)` для логотипа/LED ring.

- **Permission:** `led`
- **Аргумент:** последовательность цветов в том же формате, что и
  [`alice.set`](led-set.md).
- **Результат:** `nil` при успехе.
- **Доступность Mini:** да; требуется capability `led.ring.rgb`, frame Mini
  содержит 6 цветов.

```lua
alice.logo.set({ "#ffffff", "#ffffff", "#ffffff", "#ffffff", "#ffffff", "#ffffff" })
```
