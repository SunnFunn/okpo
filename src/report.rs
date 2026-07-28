//! Excel-отчёт для режима `--test-check`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_xlsxwriter::{Format, Workbook, Worksheet};

const HEADERS: [&str; 8] = [
    "Номер вагона",
    "Станция отправления",
    "Код станции отправления",
    "Дорога отправления",
    "Станция назначения",
    "Код станции назначения",
    "Дорога назначения",
    "Источник",
];

const COL_WIDTHS: [f64; 8] = [14.0, 28.0, 18.0, 14.0, 28.0, 18.0, 14.0, 16.0];

/// Строка итогового отчёта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    pub car_number: String,
    pub station_from: String,
    pub station_from_code: String,
    pub railway_from: String,
    pub station_to: String,
    pub station_to_code: String,
    pub railway_to: String,
    pub source: &'static str,
}

/// Путь к отчёту: `{report_dir}/test_check_{YYYY-MM-DD}.xlsx`.
pub fn report_path(report_dir: &Path, export_date: NaiveDate) -> PathBuf {
    report_dir.join(format!(
        "test_check_{}.xlsx",
        export_date.format("%Y-%m-%d")
    ))
}

/// Пишет Excel-отчёт со строками проверки.
pub fn write_report(path: &Path, rows: &[ReportRow]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("не удалось создать {}", parent.display()))?;
    }

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("Проверка")
        .context("имя листа Excel")?;

    let header_fmt = Format::new().set_bold();
    for (col, title) in HEADERS.iter().enumerate() {
        worksheet
            .write_with_format(0, col as u16, *title, &header_fmt)
            .with_context(|| format!("заголовок колонки {col}"))?;
        worksheet
            .set_column_width(col as u16, COL_WIDTHS[col])
            .ok();
    }

    for (i, row) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        write_row(worksheet, r, row)?;
    }

    let last_row = rows.len() as u32;
    let last_col = (HEADERS.len() - 1) as u16;
    worksheet.set_freeze_panes(1, 0).ok();
    if last_row > 0 {
        worksheet
            .autofilter(0, 0, last_row, last_col)
            .context("autofilter")?;
    } else {
        worksheet.autofilter(0, 0, 0, last_col).context("autofilter")?;
    }

    workbook
        .save(path)
        .with_context(|| format!("не удалось сохранить {}", path.display()))?;
    tracing::info!(
        path = %path.display(),
        rows = rows.len(),
        "Excel-отчёт записан"
    );
    Ok(())
}

fn write_row(ws: &mut Worksheet, row: u32, data: &ReportRow) -> Result<()> {
    let values = [
        data.car_number.as_str(),
        data.station_from.as_str(),
        data.station_from_code.as_str(),
        data.railway_from.as_str(),
        data.station_to.as_str(),
        data.station_to_code.as_str(),
        data.railway_to.as_str(),
        data.source,
    ];
    for (col, value) in values.iter().enumerate() {
        ws.write_string(row, col as u16, *value)
            .with_context(|| format!("ячейка row={row} col={col}"))?;
    }
    Ok(())
}
