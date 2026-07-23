use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate};
use regex::Regex;
use std::sync::OnceLock;

/// Сколько самых свежих реестров забирать за один автопрогон.
pub const PACKAGE_SIZE: usize = 4;

const MONTH_NAMES_RU: [&str; 12] = [
    "Январь",
    "Февраль",
    "Март",
    "Апрель",
    "Май",
    "Июнь",
    "Июль",
    "Август",
    "Сентябрь",
    "Октябрь",
    "Ноябрь",
    "Декабрь",
];

fn register_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)Реестр\s+(\d{1,2})\.(\d{1,2})\.+xls$")
            .expect("valid register filename regex")
    })
}

/// Папка месяца: `Июль 26`.
pub fn month_folder_name(date: NaiveDate) -> String {
    let month_name = MONTH_NAMES_RU[(date.month0()) as usize];
    let yy = date.format("%y");
    format!("{month_name} {yy}")
}

pub fn year_folder_name(date: NaiveDate) -> String {
    date.year().to_string()
}

/// Дата реестра для автопоиска: вчера по календарю «сегодня» (Москва передаётся снаружи).
pub fn target_register_date(today: NaiveDate) -> NaiveDate {
    today
        .pred_opt()
        .expect("NaiveDate::pred_opt should succeed for civil dates")
}

/// Парсит DD.MM из имени вида `Реестр 22.07..xls` / `Реестр 22.07.xls`.
pub fn parse_register_filename(filename: &str) -> Option<(u32, u32)> {
    let caps = register_date_re().captures(filename)?;
    let day: u32 = caps.get(1)?.as_str().parse().ok()?;
    let month: u32 = caps.get(2)?.as_str().parse().ok()?;
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return None;
    }
    Some((day, month))
}

/// Год для даты DD.MM относительно «сегодня».
pub fn year_for_day_month(day: u32, month: u32, today: NaiveDate) -> i32 {
    let y = today.year();
    if let Some(candidate) = NaiveDate::from_ymd_opt(y, month, day) {
        if candidate < today {
            return y;
        }
    }
    y - 1
}

fn folder_for_date(base: &Path, date: NaiveDate) -> PathBuf {
    base.join(year_folder_name(date)).join(month_folder_name(date))
}

/// Папки месяцев для пакета: месяц «вчера» и предыдущий месяц
/// (в начале месяца / года пакет перекрывает две соседние папки).
pub fn month_folders_for_package(base: &Path, today: NaiveDate) -> Vec<PathBuf> {
    let target = target_register_date(today);
    let current = folder_for_date(base, target);

    let prev_month_day = NaiveDate::from_ymd_opt(target.year(), target.month(), 1)
        .and_then(|d| d.pred_opt());

    let mut dirs = vec![current];
    if let Some(prev) = prev_month_day {
        let prev_dir = folder_for_date(base, prev);
        if prev_dir != dirs[0] {
            dirs.push(prev_dir);
        }
    }
    dirs
}

/// Реестры в одной папке с датой строго меньше `today`.
pub fn collect_registers_before_today(
    dir: &Path,
    today: NaiveDate,
) -> Result<Vec<(NaiveDate, PathBuf)>> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("не удалось прочитать каталог {}", dir.display()))?;

    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((day, month)) = parse_register_filename(name) else {
            continue;
        };
        let year = year_for_day_month(day, month, today);
        let Some(file_date) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        if file_date >= today {
            continue;
        }
        out.push((file_date, path));
    }
    Ok(out)
}

/// Среди файлов в папке выбирает реестр с максимальной датой строго меньше `today`.
pub fn pick_latest_before_today(dir: &Path, today: NaiveDate) -> Result<PathBuf> {
    let mut files = collect_registers_before_today(dir, today)?;
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .with_context(|| {
            format!(
                "в {} нет файлов реестра с датой раньше {}",
                dir.display(),
                today
            )
        })
}

/// Автопоиск пакета из [`PACKAGE_SIZE`] самых свежих реестров (дата раньше сегодняшней).
///
/// Смотрит папку месяца «вчера» и папку предыдущего месяца, чтобы в начале
/// месяца набрать 4 файла с двух соседних месяцев.
pub fn discover_latest_package(base_unc: &str, today: NaiveDate) -> Result<Vec<PathBuf>> {
    let base = PathBuf::from(base_unc);
    let dirs = month_folders_for_package(&base, today);

    tracing::info!(
        "поиск пакета из {} реестров (сегодня {}), папки: {}",
        PACKAGE_SIZE,
        today,
        dirs.iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut all: Vec<(NaiveDate, PathBuf)> = Vec::new();
    for dir in &dirs {
        if !dir.is_dir() {
            tracing::warn!("каталог реестров не найден, пропускаем: {}", dir.display());
            continue;
        }
        let found = collect_registers_before_today(dir, today)?;
        tracing::info!("в {} найдено подходящих файлов: {}", dir.display(), found.len());
        all.extend(found);
    }

    all.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    all.dedup_by(|a, b| a.1 == b.1);

    if all.len() < PACKAGE_SIZE {
        bail!(
            "для пакета нужно {PACKAGE_SIZE} файла(ов) с датой раньше {today}, найдено {}",
            all.len()
        );
    }

    let package: Vec<PathBuf> = all
        .into_iter()
        .take(PACKAGE_SIZE)
        .map(|(date, path)| {
            tracing::info!("в пакет: {} ({})", path.display(), date);
            path
        })
        .collect();

    Ok(package)
}

/// Поиск файла по имени для `--file`.
pub fn resolve_by_filename(base_unc: &str, filename: &str, today: NaiveDate) -> Result<PathBuf> {
    let base = PathBuf::from(base_unc);
    let filename = filename.trim();
    if filename.is_empty() {
        bail!("имя файла пустое");
    }

    if let Some((day, month)) = parse_register_filename(filename) {
        let year = year_for_day_month(day, month, today);
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            let candidate = folder_for_date(&base, date).join(filename);
            if candidate.is_file() {
                tracing::info!("файл найден по дате из имени: {}", candidate.display());
                return Ok(candidate);
            }
            tracing::warn!(
                "ожидаемый путь не найден ({}), пробуем обход каталогов",
                candidate.display()
            );
        }
    }

    // Fallback: обход папок текущего и предыдущего года.
    for year_offset in [0, 1] {
        let year = today.year() - year_offset;
        let year_dir = base.join(year.to_string());
        if !year_dir.is_dir() {
            continue;
        }
        for month in 1..=12 {
            let Some(date) = NaiveDate::from_ymd_opt(year, month, 1) else {
                continue;
            };
            let dir = year_dir.join(month_folder_name(date));
            let candidate = dir.join(filename);
            if candidate.is_file() {
                tracing::info!("файл найден обходом: {}", candidate.display());
                return Ok(candidate);
            }
        }
    }

    bail!(
        "файл '{}' не найден под базой {}",
        filename,
        base.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn month_folder_july_26() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 22).unwrap();
        assert_eq!(month_folder_name(d), "Июль 26");
        assert_eq!(year_folder_name(d), "2026");
    }

    #[test]
    fn target_date_on_first_of_month() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let target = target_register_date(today);
        assert_eq!(target, NaiveDate::from_ymd_opt(2026, 7, 31).unwrap());
        assert_eq!(month_folder_name(target), "Июль 26");
    }

    #[test]
    fn target_date_on_new_year() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let target = target_register_date(today);
        assert_eq!(target, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
        assert_eq!(year_folder_name(target), "2025");
        assert_eq!(month_folder_name(target), "Декабрь 25");
    }

    #[test]
    fn parse_double_dot_xls() {
        assert_eq!(
            parse_register_filename("Реестр 22.07..xls"),
            Some((22, 7))
        );
        assert_eq!(parse_register_filename("Реестр 1.12.xls"), Some((1, 12)));
        assert_eq!(parse_register_filename("readme.txt"), None);
    }

    #[test]
    fn year_for_december_in_january() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        assert_eq!(year_for_day_month(31, 12, today), 2025);
        assert_eq!(year_for_day_month(4, 1, today), 2026);
    }

    #[test]
    fn month_folders_span_two_months_at_month_start() {
        let base = PathBuf::from(r"\\share\Реестры");
        let today = NaiveDate::from_ymd_opt(2026, 8, 2).unwrap();
        let dirs = month_folders_for_package(&base, today);
        assert_eq!(dirs.len(), 2);
        assert!(dirs[0].ends_with(Path::new("2026").join("Август 26")));
        assert!(dirs[1].ends_with(Path::new("2026").join("Июль 26")));
    }

    #[test]
    fn month_folders_span_year_boundary() {
        let base = PathBuf::from(r"\\share\Реестры");
        let today = NaiveDate::from_ymd_opt(2026, 1, 3).unwrap();
        let dirs = month_folders_for_package(&base, today);
        assert_eq!(dirs.len(), 2);
        assert!(dirs[0].ends_with(Path::new("2026").join("Январь 26")));
        assert!(dirs[1].ends_with(Path::new("2025").join("Декабрь 25")));
    }

    #[test]
    fn pick_latest_before_today_from_tmp_dir() {
        let dir = tempfile_dir();
        fs::write(dir.join("Реестр 20.07..xls"), b"a").unwrap();
        fs::write(dir.join("Реестр 22.07..xls"), b"b").unwrap();
        fs::write(dir.join("Реестр 23.07..xls"), b"c").unwrap();
        fs::write(dir.join("notes.txt"), b"x").unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 7, 23).unwrap();
        let picked = pick_latest_before_today(&dir, today).unwrap();
        assert_eq!(
            picked.file_name().unwrap().to_str().unwrap(),
            "Реестр 22.07..xls"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_package_across_two_month_dirs() {
        let root = tempfile_dir();
        let july = root.join("2026").join("Июль 26");
        let august = root.join("2026").join("Август 26");
        fs::create_dir_all(&july).unwrap();
        fs::create_dir_all(&august).unwrap();

        // 2 августа: вчера = 1 августа → нужны 4 файла; часть в августе, часть в июле.
        fs::write(august.join("Реестр 01.08..xls"), b"aug1").unwrap();
        fs::write(july.join("Реестр 31.07..xls"), b"jul31").unwrap();
        fs::write(july.join("Реестр 30.07..xls"), b"jul30").unwrap();
        fs::write(july.join("Реестр 29.07..xls"), b"jul29").unwrap();
        fs::write(july.join("Реестр 28.07..xls"), b"jul28").unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 8, 2).unwrap();
        let package = discover_latest_package(root.to_str().unwrap(), today).unwrap();
        assert_eq!(package.len(), PACKAGE_SIZE);

        let names: Vec<_> = package
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "Реестр 01.08..xls",
                "Реестр 31.07..xls",
                "Реестр 30.07..xls",
                "Реестр 29.07..xls",
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }

    fn tempfile_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "okpo-discover-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
