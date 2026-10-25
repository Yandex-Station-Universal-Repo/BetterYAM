# `alice.volume.get()`

Читает текущую громкость.

- **Permission:** `volume`
- **Аргументы:** нет.
- **Результат:** целое значение процента `0..100`.
- **Доступность Mini:** capability включается, если найден ALSA control `Master`
  на card 0.

```lua
local percent = alice.volume.get()
```
