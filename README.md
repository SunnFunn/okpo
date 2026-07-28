# okpo

Ежедневный запуск `okpo-agent` на Ubuntu с reverse SOCKS (`ssh -R 3128`). По умолчанию файлы реестров на prod уже доступны с mount `/data/registers` — **SFTP не делается**. Запасной режим `--skip-register-sync` — старая выгрузка с UNC по SFTP.

## Назначение

**Дефолт (рекомендуется, mount на prod):**
1. Открывает OpenSSH-сессию: `ssh -R 3128 …` и запускает `okpo-agent` (без `--skip-register-sync`).
2. На Ubuntu agent сам копирует пакет из mount `/data/registers` в свой `data/registers/`.

**Запасной режим `--skip-register-sync` (mount недоступен):**
1. Находит пакет из **четырёх** самых свежих реестров на UNC-шаре.
2. Копирует их в `tmp/`, затем по SFTP в `remote_dir` на Ubuntu.
3. Запускает `okpo-agent` по SSH с дописанным `--skip-register-sync` (брать уже залитые файлы, не mount).

По умолчанию процесс может работать как демон и каждый день в **04:00 Europe/Moscow** запускать register (см. `config.toml`).

## Требования

- Windows-машина с **выходом в интернет** (через неё идёт SOCKS для DaData)
- Для запасного режима / `--file`: доступ к UNC-шаре (`source.base_unc`)
- Rust toolchain (`cargo`, `rustc`)
- OpenSSH-клиент в PATH (`ssh` / `ssh.exe`) — для шага `-R` + remote command
- SSH private key с доступом к Ubuntu-хосту
- На Ubuntu: для дефолта — рабочий mount `/data/registers`; для запасного — SFTP в `remote_dir`
- На Ubuntu в env okpo-agent: `OKPO_HTTP_PROXY=socks5h://127.0.0.1:3128` (или аналог)

## Конфигурация

Файл [`config.toml`](config.toml) ищется сначала в текущей директории, затем в корне проекта (`CARGO_MANIFEST_DIR`). Если файл не найден — используются значения по умолчанию из кода.

```toml
[source]
base_unc = "\\\\mskfs.rusagrotrans.ru\\Groups\\...\\Реестры"

[schedule]
timezone = "Europe/Moscow"
hour = 4
minute = 0

[ssh]
host = "10.101.139.4"
port = 22
user = "atretyakov"
private_key = "C:\\Users\\tretyakov_av\\.ssh\\id_rsa"
remote_dir = "/home/atretyakov/okpo-agent/data/registers"

[agent]
enabled = true
remote_socks_port = 3128
working_directory = "/home/atretyakov/okpo-agent"
remote_command = "OKPO_SKIP_BUILD=1 ./run.sh prod register --dadata-parse"
ssh_binary = "ssh"
```

| Секция | Поле | Описание |
|--------|------|----------|
| `source` | `base_unc` | Корневая папка реестров на шаре |
| `schedule` | `timezone` | Таймзона расписания (IANA) |
| `schedule` | `hour` / `minute` | Время ежедневного запуска |
| `ssh` | `host` / `port` / `user` | Параметры SSH |
| `ssh` | `private_key` | Путь к приватному ключу |
| `ssh` | `remote_dir` | Каталог на Ubuntu для реестров |
| `agent` | `enabled` | Запускать okpo-agent по SSH с `-R` |
| `agent` | `remote_socks_port` | Порт remote dynamic SOCKS (`ssh -R <port>`) |
| `agent` | `working_directory` | `cd` на Ubuntu перед командой |
| `agent` | `remote_command` | Базовая команда register на Ubuntu (`--skip-register-sync` дописывается только в запасном режиме) |
| `agent` | `ssh_binary` | OpenSSH-клиент (`ssh` / `ssh.exe`) |
| `test_check` | `remote_exports_dir` | Каталог JSON-экспортов на Ubuntu |
| `test_check` | `python_binary` / `script_path` | Python и скрипт SQL для `--test-check` |
| `test_check` | `report_dir` | Каталог Excel-отчёта |

## Два режима запуска

| | **Дефолт** (mount на prod) | **Запасной** `--skip-register-sync` |
|--|----------------------------|-------------------------------------|
| Когда | Mount `/data/registers` на Ubuntu работает | Mount недоступен / битый |
| На Windows | Только `ssh -R` + remote register | Отбор 4 файлов с UNC → SFTP → `ssh -R` |
| На Ubuntu (agent) | Сам sync с mount (флаг **не** передаётся) | Получает `--skip-register-sync`, берёт уже залитые в `data/registers` |
| UNC-шара с Windows | Не нужна | Нужна |

### Режим 1 — дефолт (рекомендуется)

Сборка (один раз / после правок):

```bat
cargo build --release
```

Разовый прогон:

```bat
cd C:\Users\tretyakov_av\Apps\okpo
.\target\release\okpo.exe --once
```

Демон (каждый день по `schedule` в `config.toml`):

```bat
.\target\release\okpo.exe
```

Что происходит:
1. SFTP **не** выполняется.
2. `ssh -R 3128` на prod + команда из `config.toml`, например:
   `OKPO_SKIP_BUILD=1 ./run.sh prod register --dadata-parse`
3. На Ubuntu `okpo-agent` копирует пакет из `/data/registers` (mount) в свой `data/registers/`, дальше обычный register.

Планировщик заданий Windows: программа = `okpo.exe`, аргументы = `--once`, рабочая папка = корень проекта (см. [WINDOWS_TASK_SCHEDULER.md](WINDOWS_TASK_SCHEDULER.md)).

### Режим 2 — запасной (`--skip-register-sync`)

Нужен доступ Windows к UNC (`source.base_unc`) и рабочий SFTP на Ubuntu.

```bat
cd C:\Users\tretyakov_av\Apps\okpo
.\target\release\okpo.exe --once --skip-register-sync
```

Только выгрузка файлов, без register на Ubuntu:

```bat
.\target\release\okpo.exe --once --skip-register-sync --skip-agent
```

Один файл по имени:

```bat
.\target\release\okpo.exe --file "Реестр 22.07..xls"
```

(`--file` всегда идёт по SFTP и сам дописывает `--skip-register-sync` remote-команде.)

Что происходит:
1. Поиск 4 свежих `Реестр DD.MM..xls` на UNC (логика ниже).
2. Копирование в `tmp/` → SFTP в `ssh.remote_dir`.
3. `ssh -R 3128` + та же `remote_command`, **плюс** `--skip-register-sync`, например:
   `OKPO_SKIP_BUILD=1 ./run.sh prod register --dadata-parse --skip-register-sync`

Планировщик при недоступном mount: аргументы = `--once --skip-register-sync`.

### Краткая шпаргалка команд

```bat
:: дефолт
.\target\release\okpo.exe --once

:: запасной SFTP
.\target\release\okpo.exe --once --skip-register-sync

:: только SFTP, без agent
.\target\release\okpo.exe --once --skip-register-sync --skip-agent

:: один файл (SFTP + agent --skip-register-sync)
.\target\release\okpo.exe --file "Реестр 22.07..xls"

:: проверка расхождений (--test-check)
.\target\release\okpo.exe --test-check
```

## Режим `--test-check`

Одноразовая проверка: вагоны, которые по дислокации едут на одну станцию, но уже назначены диспетчером под погрузку на другую, плюс сверка с последним JSON-экспортом парсинга.

**Что делает:**
1. По SFTP скачивает самый свежий `register_export_YYYY-MM-DD.json` из `test_check.remote_exports_dir` на Ubuntu в локальный `tmp/`.
2. Запускает `test/assignments.py` (два SQL: реестр назначений + накладные) через Python/`pyodbc`.
3. Дедуплицирует номера вагонов: приоритет `registers` → `invoices` → JSON-экспорт.
4. Пишет Excel `tmp/test_check_YYYY-MM-DD.xlsx` (станции/коды/дороги из источника строки).

**Требования:** `pyodbc` + ODBC Driver 18 for SQL Server; Windows под доменным пользователем с доступом к `MSKASUVPL` / `ASUVP_RAT`. Режим **не** запускает okpo-agent и **не** заливает реестры на прод.

```bat
cargo run -- --test-check
cargo run --release -- --test-check
.\target\release\okpo.exe --test-check
```

Секция `[test_check]` в `config.toml`:

| Поле | Описание |
|------|----------|
| `remote_exports_dir` | Каталог JSON на Ubuntu |
| `export_prefix` | Префикс имени (`register_export`) |
| `python_binary` | Интерпретатор Python |
| `script_path` | Путь к `test/assignments.py` |
| `registers_days` / `invoices_days` | Глубина SQL-выборок |
| `report_dir` | Куда писать Excel (обычно `tmp`) |

## Логика выбора файлов (только режим `--skip-register-sync` / `--file`)

Время считается в таймзоне из `config.toml` (по умолчанию Москва).

1. Рассматриваются реестры с датой **строго меньше сегодняшней**.
2. Просматриваются **две** папки месяцев:
   - месяц даты «вчера» (год `YYYY`, папка `{РусскоеИмя} {YY}`, например `Август 26`);
   - **предыдущий** месяц (в начале месяца / года — соседняя папка, в т.ч. `Декабрь` прошлого года).
3. Среди файлов вида `Реестр DD.MM..xls` / `Реестр DD.MM.xls` выбираются **4 самых поздних** по дате.
4. Если найдено меньше 4 файлов — ошибка (прогон завершается с ненулевым кодом).

Имена на Ubuntu совпадают с исходными именами файлов.

## Поведение при существующем файле на Ubuntu (SFTP)

Перед записью проверяется наличие remote-файла. Если он уже есть — в лог пишется предупреждение, файл **перезаписывается**. Это не считается ошибкой.

## Сборка и прочие команды

```bat
:: проверка и сборка
cargo check
cargo build
cargo build --release
cargo test

:: справка по флагам
cargo run -- --help

:: демон (дефолт: только ssh -R + agent)
cargo run --release
.\target\release\okpo.exe
```

Уровень логов можно задать через `RUST_LOG` (по умолчанию `info`):

```bat
set RUST_LOG=debug
cargo run -- --once
```

Логи каждого запуска пишутся в `okpo-task.log` в корне проекта (рядом с `config.toml`).
Файл **перезаписывается** при каждом прогоне — хранится только последний запуск.

## Режимы CLI (сводка)

| Режим | Команда | Поведение |
|-------|---------|-----------|
| Демон (дефолт) | `okpo` | Расписание → только `ssh -R` + agent (mount на Ubuntu) |
| Разовый (дефолт) | `okpo --once` | Только `ssh -R` + agent |
| Запасной SFTP | `… --skip-register-sync` | UNC → SFTP, agent с `--skip-register-sync` |
| Ручная загрузка | `okpo --file "…"` | Один файл по SFTP + agent с `--skip-register-sync` |
| Без agent | `… --skip-agent` | Не запускать remote register |
| Проверка | `okpo --test-check` | JSON с прод + 2 SQL + Excel-отчёт (без agent) |

Флаг `--file` одноразовый: после прогона процесс завершается. Не комбинируется с `--once`.

### Почему два SSH-канала (в режиме `--skip-register-sync`)

| Шаг | Как | Зачем |
|-----|-----|-------|
| SFTP | russh в процессе `okpo` | Выгрузка файлов, если mount недоступен |
| `-R` + register | системный `ssh` | Remote dynamic SOCKS (`-R 3128`); russh так не умеет |

В **дефолтном** режиме остаётся только шаг `-R` + register.

Пока идёт `remote_command`, туннель жив; после exit register сессия закрывается — systemd-таймер на prod без живого `-R` для DaData не подходит.

## Эксплуатация

- Для срабатывания встроенного таймера процесс должен быть **запущен и не завершён** (сессия пользователя / служба / автозагрузка).
- Альтернатива: Windows Task Scheduler с ежедневным запуском `okpo.exe --once` (см. [WINDOWS_TASK_SCHEDULER.md](WINDOWS_TASK_SCHEDULER.md)). Учтите: register может идти долго — увеличьте лимит времени задачи.
- ПК должен быть **онлайн** на время register (SOCKS через Windows).
- Временные файлы (`tmp/`) появляются только в режиме SFTP и удаляются после **успешной** загрузки на Ubuntu.
