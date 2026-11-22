<p align="center"><img src="assets/icon.png" width="112" alt=""></p>

<h1 align="center">Altherra Launcher</h1>

<p align="center">Лаунчер Minecraft-сервера <b>Altherra</b>. Швидкий, нативний, без зайвого.</p>

---

- **Ліцензія й без неї.** Вхід через Microsoft або просто за ніком; перемикання між акаунтами в один клік.
- **Ставить усе сам.** Java, Minecraft, Fabric і моди сервера: нічого не треба шукати й копіювати руками.
- **Завжди актуальна збірка.** Коли на сервері оновлюються моди, лаунчер підтягне зміни перед наступним запуском.
- **Легкий.** Один файл на Rust + [Slint](https://slint.dev) без браузера всередині: відкривається миттєво й майже не їсть памʼяті.
- **Надійний.** Кожен файл перевіряється за контрольною сумою, а невдалі завантаження повторюються. Лаунчер ходить в інтернет лише через HTTPS і не дає серверу збірки виконати щось поза грою.

## Дві версії

| | **Altherra** | **Altherra Offline** |
|---|---|---|
| Вхід з ліцензією Microsoft | ✅ | — |
| Гра за ніком | після входу з ліцензійним акаунтом | ✅ одразу |
| Код входу Microsoft і Client ID | є | немає зовсім |
| Оформлення | бірюзове | червоне |

Обидві ставлять Java, Minecraft, Fabric і моди сервера, оновлюються самі й можуть стояти поруч.

Обидві завжди ставлять мод **Altherra Client** (його код — у [`client-mod/`](client-mod)): після кіку чи обриву звʼязку гра сама перепідключається до сервера, а кнопка «Вийти з гри» в меню паузи закриває Minecraft. Якщо на сервері немає Fabric API, лаунчер бере його з Modrinth.

### Гілки
- **`main`** — основна версія **Altherra**, релізи `launcher-v…` (саме вони «останні» на сторінці Releases).
- **`offline`** — **Altherra Offline**, релізи `offline-v…`. Відрізняється лише тим, що збирається без входу Microsoft і має своє оформлення; код спільний, зміни з `main` переносяться сюди.

## Завантажити

Основна версія — [останній реліз](../../releases/latest). Altherra Offline — у [релізах `offline-v…`](../../releases):

| Система | Altherra | Altherra Offline |
|---|---|---|
| Windows 10/11 | `Altherra-Setup-windows-x64.exe` | `Altherra-Offline-Setup-windows-x64.exe` |
| Linux (glibc і musl) | `Altherra-Setup-linux-x64.run` | `Altherra-Offline-Setup-linux-x64.run` |
| macOS 11+ (Apple Silicon та Intel) | `Altherra-macos.zip` | `Altherra-Offline-macos.zip` |

Інсталятор для Linux запускається так: `sh Altherra-Setup-linux-x64.run`.

Встановлений лаунчер оновлюється сам: коли виходить нова версія, зверху зʼявляється кнопка «Оновити». Оновлення підписані ключем Ed25519, тож підмінений файл лаунчер не встановить.

## Як видалити

Видаляється сам лаунчер. Світи, налаштування, скріншоти й акаунти лишаються в теці гри — її можна видалити окремо (останній крок).

### Windows
1. **Параметри → Програми → Встановлені програми** (або **Панель керування → Програми та компоненти**).
2. Знайди **Altherra** або **Altherra Offline** → **Видалити**.

Або: меню «Пуск» → тека **Altherra** → **Видалити Altherra**.

### Linux
- У меню програм: правий клік на **Altherra** → **Видалити Altherra**.
- Або в терміналі:
  ```bash
  sh ~/.local/share/altherra-launcher/uninstall.run --uninstall   # Altherra
  sh ~/.local/share/altherra-offline/uninstall.run --uninstall    # Altherra Offline
  ```

### macOS
Перетягни **Altherra** (або **Altherra Offline**) із «Програм» у Кошик.

### Файли гри
Якщо хочеш прибрати й світи, налаштування та акаунти:

| Система | Тека |
|---|---|
| Windows | `%APPDATA%\.altherra` (встав у рядок адреси Провідника) |
| Linux | `~/.altherra` |
| macOS | `~/Library/Application Support/altherra` |

Тека спільна для обох версій — видаляй її, лише якщо прибираєш обидві.

## Збирання

```bash
cargo build --release                        # Altherra
cargo build --release --no-default-features  # Altherra Offline
```

---

Не є офіційним продуктом Minecraft. Не схвалено Mojang чи Microsoft і не повʼязано з ними. Шрифти Unbounded, Manrope і JetBrains Mono — за ліцензією SIL OFL 1.1 (`ui/fonts`). Made with Slint.
