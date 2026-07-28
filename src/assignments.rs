//! Мост Rust → Python (`test/assignments.py`): два SQL-запроса, JSON на stdout.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::config::{Config, TestCheckConfig};

const STATS_PREFIX: &str = "OKPO_ASSIGNMENTS_STATS=";

#[derive(Debug, Clone, Deserialize)]
pub struct AssignmentRow {
    #[serde(rename = "CarNumber")]
    pub car_number: String,
    #[serde(rename = "StationFrom", default)]
    pub station_from: Option<String>,
    #[serde(rename = "StationFromCode", default)]
    pub station_from_code: Option<String>,
    #[serde(rename = "RailWayFrom", default)]
    pub railway_from: Option<String>,
    #[serde(rename = "StationTo", default)]
    pub station_to: Option<String>,
    #[serde(rename = "StationToCode", default)]
    pub station_to_code: Option<String>,
    #[serde(rename = "RailWayTo", default)]
    pub railway_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssignmentsOutput {
    pub registers: Vec<AssignmentRow>,
    pub invoices: Vec<AssignmentRow>,
}

fn resolve_script(tc: &TestCheckConfig) -> Result<PathBuf> {
    let configured = PathBuf::from(tc.script_path.trim());
    let candidates = [
        Config::project_root().join(&configured),
        PathBuf::from(".").join(&configured),
        configured.clone(),
    ];
    for path in &candidates {
        if path.is_file() {
            return Ok(path.canonicalize().unwrap_or_else(|_| path.clone()));
        }
    }
    bail!(
        "скрипт assignments не найден: {} (искали относительно CARGO_MANIFEST_DIR и cwd)",
        tc.script_path
    )
}

/// Запускает Python-скрипт и парсит JSON со stdout.
pub fn fetch_assignments(tc: &TestCheckConfig) -> Result<AssignmentsOutput> {
    let script = resolve_script(tc)?;
    let workdir = Config::project_root();

    tracing::info!(
        python = %tc.python_binary,
        script = %script.display(),
        registers_days = tc.registers_days,
        invoices_days = tc.invoices_days,
        "запуск assignments.py"
    );

    let output = Command::new(&tc.python_binary)
        .arg(&script)
        .arg("--registers-days")
        .arg(tc.registers_days.to_string())
        .arg("--invoices-days")
        .arg(tc.invoices_days.to_string())
        .current_dir(&workdir)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| {
            format!(
                "не удалось запустить `{} {}` (нужен Python + pyodbc)",
                tc.python_binary,
                script.display()
            )
        })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(json) = line.strip_prefix(STATS_PREFIX) {
            tracing::info!(stats = %json, "assignments: статистика SQL");
        } else {
            tracing::debug!(line = %line, "assignments stderr");
        }
    }

    if !output.status.success() {
        bail!(
            "assignments.py завершился с {:?}:\n{}",
            output.status.code(),
            stderr.trim()
        );
    }

    let parsed: AssignmentsOutput = serde_json::from_slice(&output.stdout)
        .context("не удалось разобрать JSON stdout assignments.py")?;

    tracing::info!(
        registers = parsed.registers.len(),
        invoices = parsed.invoices.len(),
        "assignments: данные получены"
    );
    Ok(parsed)
}

/// Проверка, что путь к скрипту выглядит как файл (для тестов без запуска Python).
#[cfg(test)]
pub fn script_path_candidates(script_path: &str) -> Vec<PathBuf> {
    let configured = PathBuf::from(script_path);
    vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&configured),
        PathBuf::from(".").join(&configured),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_exists_in_repo() {
        let found = script_path_candidates("test/assignments.py")
            .into_iter()
            .any(|p| p.is_file());
        assert!(found, "test/assignments.py должен лежать в репозитории");
    }

    #[test]
    fn parse_assignments_json() {
        let raw = r#"{
            "registers": [{"CarNumber":"12345678","StationFrom":"A","StationFromCode":"1","RailWayFrom":"МСК","StationTo":"B","StationToCode":"2","RailWayTo":"ЮВС"}],
            "invoices": [{"CarNumber":"87654321","StationFrom":null,"StationFromCode":null,"RailWayFrom":null,"StationTo":"C","StationToCode":"3","RailWayTo":"МСК"}]
        }"#;
        let out: AssignmentsOutput = serde_json::from_str(raw).unwrap();
        assert_eq!(out.registers.len(), 1);
        assert_eq!(out.invoices[0].car_number, "87654321");
        assert!(out.invoices[0].station_from.is_none());
    }
}
