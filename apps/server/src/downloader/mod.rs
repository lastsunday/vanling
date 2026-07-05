use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bzip2::read::BzDecoder;
use tar::Archive;

use api::config::{AsrModel, Config as AppConfig, LlmModel, TtsModel, VadModel};
use dialoguer::Select;
use include_dir::{Dir, include_dir};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const MAX_CONCURRENT_DOWNLOADS: usize = 4;
const MAX_DOWNLOAD_RETRIES: u32 = 3;
const RETRY_BASE_DELAY_MS: u64 = 1000;

static MANIFESTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/downloader/manifests");

const DEFAULT_MIRRORS: &[&str] = &["https://hf-mirror.com"];
const MANIFESTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/downloader/manifests");

#[derive(Deserialize)]
#[allow(dead_code)]
struct ConfigMeta {
    category: String,
    #[serde(rename = "model")]
    model_name: String,
}

#[derive(Deserialize)]
struct ModelEntry {
    config: Option<ConfigMeta>,
    default_variant: Option<String>,
    variants: HashMap<String, Variant>,
}

#[derive(Deserialize)]
struct Variant {
    #[serde(default)]
    files: Vec<FileEntry>,
    #[serde(default)]
    archives: Vec<ArchiveEntry>,
    #[serde(default)]
    #[allow(dead_code)]
    prompt_text: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ExtractEntry {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

fn extract_target_path(dest_dir: &Path, entry: &ExtractEntry) -> PathBuf {
    match &entry.target {
        Some(t) => dest_dir.join(t),
        None => {
            let name = Path::new(&entry.path)
                .file_name()
                .unwrap_or(OsStr::new(&entry.path));
            dest_dir.join(name)
        }
    }
}

fn archive_report_path(archive: &ArchiveEntry) -> String {
    format!("{}.tar.bz2", archive.path.trim_end_matches('/'))
}

#[derive(Deserialize)]
struct ArchiveEntry {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    path: String,
    extract: Vec<ExtractEntry>,
}

#[derive(Deserialize)]
struct FileEntry {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    path: String,
}

#[derive(Deserialize)]
struct OverrideEntry {
    url: String,
    #[serde(default)]
    sha256: Option<String>,
}

type OverrideMap = HashMap<String, OverrideEntry>;

#[derive(Serialize, Deserialize)]
struct Report {
    completed_at: String,
    base_dir: String,
    files: Vec<ReportFile>,
}

#[derive(Serialize, Deserialize)]
struct ReportFile {
    path: String,
    size: u64,
    sha256: String,
    status: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    category: Option<&str>,
    model: Option<&str>,
    variant: Option<&str>,
    data_dir: &Path,
    quiet: bool,
    mirrors: &[String],
    overrides: Option<&str>,
    config_path: Option<&PathBuf>,
    all: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let has_filters = category.is_some() || model.is_some() || variant.is_some();
    if all && has_filters {
        return Err("--all cannot be used with CATEGORY, MODEL, or VARIANT".into());
    }
    let client = Client::new();

    let override_map = match overrides {
        Some(src) => Some(load_overrides(src).await?),
        None => None,
    };

    let targets = if has_filters || all {
        Vec::new()
    } else {
        let cfg_path = config_path
            .cloned()
            .or_else(|| find_config().filter(|p| p.exists()));

        let figment = match &cfg_path {
            Some(p) => AppConfig::load(std::slice::from_ref(p))?,
            None => AppConfig::load(&[] as &[std::path::PathBuf])?,
        };
        let cfg = AppConfig::new(&figment)?;
        let t = config_to_targets(&cfg);

        if t.is_empty() {
            if !quiet {
                eprintln!("No enabled models in configuration. Nothing to download.");
            }
            return Ok(());
        }

        if !quiet {
            eprintln!("Config selects {} model(s)", t.len());
            for (cat, m, var) in &t {
                if let Some(v) = var {
                    eprintln!("  {cat}/{m} (variant: {v})");
                } else {
                    eprintln!("  {cat}/{m} (default variant)");
                }
            }
            eprintln!();
        }

        t
    };

    let mut report_files = Vec::new();

    for cat_dir in MANIFESTS.dirs() {
        let cat_name = dir_name(cat_dir);
        if let Some(c) = category
            && c != cat_name
        {
            continue;
        }

        for file_entry in cat_dir.files() {
            if file_entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }

            let model_name = file_entry.path().file_stem().unwrap().to_str().unwrap();
            if let Some(m) = model
                && m != model_name
            {
                continue;
            }

            if !all
                && !has_filters
                && !targets
                    .iter()
                    .any(|(c, m, _)| c == cat_name && m == model_name)
            {
                continue;
            }

            let entry: ModelEntry = serde_json::from_slice(file_entry.contents())?;

            let variants: HashMap<&str, &Variant> = if all {
                entry
                    .variants
                    .iter()
                    .map(|(k, v)| (k.as_str(), v))
                    .collect()
            } else {
                let effective_variant = variant.or_else(|| {
                    targets
                        .iter()
                        .find(|(c, m, _)| c == cat_name && m == model_name)
                        .and_then(|(_, _, v)| v.as_deref())
                });
                resolve_variants(&entry, effective_variant)
            };
            for (v_name, v) in &variants {
                if !quiet {
                    eprintln!("[{cat_name}/{model_name}/{v_name}]");
                }

                let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
                let mut set = JoinSet::new();

                for file in &v.files {
                    let dest = data_dir.join(&file.path);

                    let (file_url, file_sha256) =
                        match override_map.as_ref().and_then(|m| m.get(&file.path)) {
                            Some(ov) => (
                                ov.url.clone(),
                                ov.sha256.clone().or_else(|| file.sha256.clone()),
                            ),
                            None => (file.url.clone(), file.sha256.clone()),
                        };

                    let cl = client.clone();
                    let mir = mirrors.to_vec();
                    let fpath = file.path.clone();
                    let sem = sem.clone();

                    set.spawn(async move {
                        let _permit = sem.acquire_owned().await.unwrap();
                        let result = download_file(
                            &cl,
                            &file_url,
                            &dest,
                            file_sha256.as_deref(),
                            &mir,
                            quiet,
                        )
                        .await
                        .map_err(|e| format!("{e}"));
                        (fpath, result)
                    });
                }

                while let Some(res) = set.join_next().await {
                    let (fpath, result) = res?;
                    match result {
                        Ok((size, sha256)) => {
                            if !quiet {
                                eprintln!("  {}: {} bytes, sha256: {sha256} OK", fpath, size);
                            }
                            report_files.push(ReportFile {
                                path: fpath.clone(),
                                size,
                                sha256: sha256.clone(),
                                status: "ok".into(),
                            });
                        }
                        Err(msg) => {
                            eprintln!("  FAIL: {} ({msg})", fpath);
                            report_files.push(ReportFile {
                                path: fpath,
                                size: 0,
                                sha256: String::new(),
                                status: format!("failed: {msg}"),
                            });
                        }
                    }
                }

                for archive in &v.archives {
                    let dest_dir = data_dir.join(&archive.path);
                    let archive_file = dest_dir.with_file_name(format!(
                        "{}.tar.bz2",
                        dest_dir
                            .file_name()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or("model")
                    ));
                    let all_exist = if cat_name == "reference" {
                        archive.extract.iter().all(|f| {
                            let ext = Path::new(&f.path)
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("wav");
                            dest_dir.join(format!("{}.{}", v_name, ext)).exists()
                        })
                    } else {
                        archive
                            .extract
                            .iter()
                            .all(|f| extract_target_path(&dest_dir, f).exists())
                    };
                    if all_exist {
                        if !quiet {
                            eprintln!("  ARCHIVE {}: all check files exist, skip", archive.path);
                        }
                        let (size, sha256) = if archive_file.exists() {
                            sha256_file(&archive_file)?
                        } else {
                            (0, String::new())
                        };
                        report_files.push(ReportFile {
                            path: archive_report_path(archive),
                            size,
                            sha256,
                            status: "ok".into(),
                        });
                        continue;
                    }

                    if !quiet {
                        eprintln!("  ARCHIVE {}: downloading ...", archive.path);
                    }

                    let result = download_file(
                        &client,
                        &archive.url,
                        &archive_file,
                        archive.sha256.as_deref(),
                        mirrors,
                        quiet,
                    )
                    .await;

                    match result {
                        Ok((size, sha256)) => {
                            if !quiet {
                                eprintln!(
                                    "  {}: {} bytes, sha256: {sha256} OK",
                                    archive_report_path(archive),
                                    size
                                );
                                eprintln!(
                                    "  ARCHIVE {}: extracting ...",
                                    archive_report_path(archive)
                                );
                            }
                            match extract_tar_bz2(&archive_file, &dest_dir, &archive.extract, quiet)
                            {
                                Ok(()) => {
                                    if cat_name == "reference" {
                                        for f in &archive.extract {
                                            let orig_path = extract_target_path(&dest_dir, f);
                                            if orig_path.exists() {
                                                let ext = Path::new(&f.path)
                                                    .extension()
                                                    .and_then(|e| e.to_str())
                                                    .unwrap_or("wav");
                                                let new_name = format!("{}.{}", v_name, ext);
                                                let new_path = dest_dir.join(&new_name);
                                                if orig_path != new_path {
                                                    let _ = std::fs::rename(&orig_path, &new_path);
                                                }
                                            }
                                        }
                                    }
                                    report_files.push(ReportFile {
                                        path: archive_report_path(archive),
                                        size,
                                        sha256,
                                        status: "ok".into(),
                                    });
                                }
                                Err(e) => {
                                    let msg = format!("extraction failed: {e}");
                                    eprintln!("  FAIL: {} ({msg})", archive_report_path(archive));
                                    report_files.push(ReportFile {
                                        path: archive_report_path(archive),
                                        size: 0,
                                        sha256: String::new(),
                                        status: format!("failed: {msg}"),
                                    });
                                }
                            }
                        }
                        Err(msg) => {
                            eprintln!("  FAIL: {} ({msg})", archive_report_path(archive));
                            report_files.push(ReportFile {
                                path: archive_report_path(archive),
                                size: 0,
                                sha256: String::new(),
                                status: format!("failed: {msg}"),
                            });
                        }
                    }
                }
            }
        }
    }

    let total = report_files.len();
    let ok = report_files.iter().filter(|f| f.status == "ok").count();
    let failed = total - ok;

    if !quiet {
        eprintln!(
            "\nDownload complete: {ok} OK, {failed} failed. Report written to {}/download-report.json",
            data_dir.display()
        );
    }

    let failed_paths: Vec<String> = report_files
        .iter()
        .filter(|f| f.status != "ok")
        .map(|f| f.path.clone())
        .collect();

    if failed > 0 {
        eprintln!("  FAILED: {}", failed_paths.join(", "));
    }

    let report = Report {
        completed_at: jiff::Zoned::now().to_string(),
        base_dir: data_dir.to_string_lossy().into(),
        files: report_files,
    };

    std::fs::create_dir_all(data_dir)?;
    std::fs::write(
        data_dir.join("download-report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;

    if failed > 0 {
        return Err(format!(
            "{failed} file(s) failed to download: {}",
            failed_paths.join(", ")
        )
        .into());
    }

    Ok(())
}

pub fn list(category: Option<&str>, json: bool) {
    #[derive(Serialize)]
    struct ModelInfo {
        model: String,
        default_variant: Option<String>,
        variants: Vec<String>,
    }

    let mut output: HashMap<String, Vec<ModelInfo>> = HashMap::new();

    for cat_dir in MANIFESTS.dirs() {
        let cat_name = dir_name(cat_dir).to_string();
        if let Some(c) = category
            && c != cat_name
        {
            continue;
        }

        let models = output.entry(cat_name).or_default();

        for file_entry in cat_dir.files() {
            if file_entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }

            let model_name = file_entry
                .path()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let Ok(entry) = serde_json::from_slice::<ModelEntry>(file_entry.contents()) else {
                continue;
            };

            models.push(ModelInfo {
                model: model_name,
                default_variant: entry.default_variant,
                variants: entry.variants.into_keys().collect(),
            });
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return;
    }

    for (cat, models) in &output {
        println!("{cat}");
        let count = models.len();
        for (i, m) in models.iter().enumerate() {
            let is_last = i == count - 1;
            let prefix = if is_last {
                "  └── "
            } else {
                "  ├── "
            };
            let child_prefix = if is_last { "      " } else { "  │   " };

            let has_variants = m.variants.len() > 1 || !m.variants.contains(&"default".into());
            println!("{prefix}{}", m.model);

            if has_variants {
                let vcount = m.variants.len();
                for (j, v) in m.variants.iter().enumerate() {
                    let v_is_last = j == vcount - 1;
                    let v_prefix = if v_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    let suffix = if m.default_variant.as_deref() == Some(v) {
                        " (default)"
                    } else {
                        ""
                    };
                    println!("{child_prefix}{v_prefix}{v}{suffix}");
                }
            }
        }
    }
}

async fn load_overrides(src: &str) -> Result<OverrideMap, Box<dyn std::error::Error>> {
    let bytes: Vec<u8> = if src.starts_with("http://") || src.starts_with("https://") {
        Client::new().get(src).send().await?.bytes().await?.to_vec()
    } else {
        std::fs::read(src)?
    };
    Ok(serde_json::from_slice(&bytes)?)
}

fn generate_urls(primary: &str, mirrors: &[String]) -> Vec<String> {
    let mut urls = vec![primary.to_string()];

    if let Some(path) = primary.strip_prefix("https://huggingface.co/") {
        let list: Vec<&str> = if mirrors.is_empty() {
            DEFAULT_MIRRORS.to_vec()
        } else {
            mirrors.iter().map(|s| s.as_str()).collect()
        };
        for m in list {
            urls.push(format!("{}/{}", m.trim_end_matches('/'), path));
        }
    }

    urls
}

async fn download_file(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    mirrors: &[String],
    quiet: bool,
) -> Result<(u64, String), Box<dyn std::error::Error>> {
    if dest.exists() {
        let (size, actual) = sha256_file(dest)?;
        if let Some(expected) = expected_sha256 {
            if actual != expected {
                std::fs::remove_file(dest)?;
                if !quiet {
                    eprintln!("  SHA256 mismatch, re-downloading {}", dest.display());
                }
            } else {
                return Ok((size, actual));
            }
        } else {
            return Ok((size, actual));
        }
    }

    std::fs::create_dir_all(dest.parent().unwrap())?;
    let candidates = generate_urls(url, mirrors);

    for (i, candidate) in candidates.iter().enumerate() {
        if i > 0 && !quiet {
            eprintln!("  RETRY from {candidate}");
        }

        let result = try_download_url(client, candidate, dest, expected_sha256, quiet).await;
        if result.is_ok() {
            return result;
        }
    }

    Err("All download attempts failed".into())
}

async fn try_download_url(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    quiet: bool,
) -> Result<(u64, String), Box<dyn std::error::Error>> {
    let tmp = PathBuf::from(format!("{}.tmp.{}", dest.display(), std::process::id()));

    for attempt in 0..MAX_DOWNLOAD_RETRIES {
        let _ = std::fs::remove_file(&tmp);

        let result = try_download_attempt(client, url, &tmp, dest, expected_sha256, quiet).await;
        if let Ok(r) = result {
            std::fs::rename(&tmp, dest)?;
            return Ok(r);
        }

        let err_msg = {
            let e = result.unwrap_err();
            let msg = e.to_string();
            if msg.contains("SHA256 mismatch") || attempt + 1 == MAX_DOWNLOAD_RETRIES {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            msg
        };

        let delay = RETRY_BASE_DELAY_MS * 2u64.pow(attempt);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        if !quiet {
            eprintln!(
                "  Retry {}/{} for {} (error: {})",
                attempt + 2,
                MAX_DOWNLOAD_RETRIES,
                url,
                err_msg,
            );
        }
    }

    unreachable!()
}

async fn try_download_attempt(
    client: &Client,
    url: &str,
    tmp: &Path,
    dest: &Path,
    expected_sha256: Option<&str>,
    quiet: bool,
) -> Result<(u64, String), Box<dyn std::error::Error>> {
    let mut hasher = sha2::Sha256::new();
    let mut downloaded = 0u64;

    let mut resp = client.get(url).send().await?;
    let total_size = resp.content_length().unwrap_or(0);

    let use_bar = !quiet && total_size > 0 && std::io::stderr().is_terminal();

    let pb = if use_bar {
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg:.bold.dim} {bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}, eta {eta})")
                .unwrap()
                .progress_chars("=>-"),
        );
        pb.set_message(
            dest.file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("")
                .to_string(),
        );
        Some(pb)
    } else {
        if !quiet && total_size > 0 {
            let fname = dest
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("")
                .to_string();
            eprintln!("  {fname} ({total_size} bytes)");
        }
        None
    };

    let mut file = std::fs::File::create(tmp)?;
    let mut last_pct = 0u32;
    while let Some(chunk) = resp.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        if let Some(ref pb) = pb {
            pb.set_position(downloaded);
        } else if !quiet && total_size > 0 {
            let pct = (downloaded * 100 / total_size) as u32;
            if pct - last_pct >= 10 {
                let fname = dest
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or("")
                    .to_string();
                eprintln!("  {fname} ... {pct}%");
                last_pct = pct;
            }
        }
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    } else if !quiet && total_size > 0 {
        let fname = dest
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("")
            .to_string();
        eprintln!("  {fname} ... done");
    }

    let actual = hex::encode(hasher.finalize());

    if let Some(expected) = expected_sha256
        && actual != *expected
    {
        return Err(format!("SHA256 mismatch: expected {expected}, got {actual}").into());
    }

    Ok((downloaded, actual))
}

fn sha256_file(path: &Path) -> Result<(u64, String), Box<dyn std::error::Error>> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 8192];
    let mut total = 0u64;

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }

    Ok((total, hex::encode(hasher.finalize())))
}

fn extract_tar_bz2(
    archive_path: &Path,
    dest: &Path,
    files: &[ExtractEntry],
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(archive_path)?;
    let decoder = BzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    std::fs::create_dir_all(dest)?;

    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let dir_prefixes: Vec<&str> = paths.iter().filter(|f| !f.contains('.')).copied().collect();
    let entry_map: HashMap<&str, &ExtractEntry> =
        files.iter().map(|e| (e.path.as_str(), e)).collect();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let stripped: PathBuf = path.components().skip(1).collect();
        if stripped.as_os_str().is_empty() {
            continue;
        }

        let stripped_str = stripped.to_string_lossy();
        let matched_entry = paths
            .iter()
            .find(|f| stripped == Path::new(f))
            .and_then(|f| entry_map.get(f));
        let matched_dir = dir_prefixes.iter().any(|p| stripped_str.starts_with(p));

        if matched_entry.is_none() && !matched_dir {
            continue;
        }

        if entry.header().entry_type().is_dir() {
            let dest_path = if let Some(e) = matched_entry
                && let Some(t) = &e.target
            {
                dest.join(t)
            } else {
                dest.join(&stripped)
            };
            std::fs::create_dir_all(&dest_path)?;
        } else {
            let dest_path = if let Some(e) = matched_entry
                && let Some(t) = &e.target
            {
                dest.join(t)
            } else if matched_dir {
                dest.join(&stripped)
            } else {
                let name = stripped.file_name().map(Path::new).unwrap_or(&stripped);
                dest.join(name)
            };
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&dest_path)?;

            if !quiet {
                let (size, actual_sha) = sha256_file(&dest_path)?;
                let fname = dest_path
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or("");

                if let Some(entry) = matched_entry {
                    if let Some(expected) = &entry.sha256
                        && actual_sha != *expected
                    {
                        eprintln!(
                            "  {fname}: {} bytes, SHA256 mismatch (expected {expected}, got {actual_sha})",
                            size,
                        );
                    } else {
                        eprintln!("  {fname}: {size} bytes, sha256: {actual_sha} OK");
                    }
                } else {
                    eprintln!("  {fname}: {size} bytes, sha256: {actual_sha} OK");
                }
            }
        }
    }

    Ok(())
}

fn write_checksums_to_manifests(
    updates: &[(PathBuf, String, String)],
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Path::new(MANIFESTS_DIR);

    let mut grouped: HashMap<&Path, Vec<(&str, &str)>> = HashMap::new();
    for (man, path, sha) in updates {
        grouped
            .entry(man.as_path())
            .or_default()
            .push((path.as_str(), sha.as_str()));
    }

    for (rel, entries) in &grouped {
        let full = base.join(rel);
        let mut val: serde_json::Value = serde_json::from_slice(&std::fs::read(&full)?)?;
        for (target_path, sha) in entries {
            set_sha256(&mut val, target_path, sha);
        }
        std::fs::write(&full, serde_json::to_string_pretty(&val)?)?;
    }

    Ok(())
}

fn set_sha256(val: &mut serde_json::Value, target: &str, sha: &str) {
    match val {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(p)) = map.get("path")
                && p == target
            {
                map.insert("sha256".into(), serde_json::Value::String(sha.into()));
                return;
            }
            for v in map.values_mut() {
                set_sha256(v, target, sha);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                set_sha256(v, target, sha);
            }
        }
        _ => {}
    }
}

fn resolve_variants<'a>(
    entry: &'a ModelEntry,
    variant: Option<&'a str>,
) -> HashMap<&'a str, &'a Variant> {
    match variant {
        Some(v) => {
            let mut map = HashMap::new();
            if let Some(var) = entry.variants.get(v) {
                map.insert(v, var);
            }
            map
        }
        None => {
            if let Some(default) = entry.default_variant.as_deref() {
                let mut map = HashMap::new();
                if let Some(var) = entry.variants.get(default) {
                    map.insert(default, var);
                }
                map
            } else {
                entry
                    .variants
                    .iter()
                    .map(|(k, v)| (k.as_str(), v))
                    .collect()
            }
        }
    }
}

fn dir_name<'a>(dir: &'a Dir<'a>) -> &'a str {
    dir.path().file_name().unwrap().to_str().unwrap()
}

struct ModelInfo {
    display: String,
    toml_model: String,
    default_variant: String,
    variants: Vec<String>,
}

fn find_config_inner(chobits_config: Option<String>) -> Option<PathBuf> {
    if let Some(path) = chobits_config {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }
    let p = PathBuf::from("application.toml");
    Some(if p.exists() {
        p
    } else {
        PathBuf::from("application.toml")
    })
}

fn find_config() -> Option<PathBuf> {
    find_config_inner(std::env::var("CHOBITS_CONFIG").ok())
}

fn load_selections(path: &Path) -> HashMap<String, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let mut map = HashMap::new();
    let mut in_global = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_global = line.trim_end() == "[global]";
            continue;
        }
        if !in_global || !line.contains('=') || line.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim();
            let val = line[eq_pos + 1..].trim().trim_matches('"');
            if key.ends_with("_model") || key.ends_with("_variant") || key.ends_with("_path") {
                map.insert(key.to_string(), val.to_string());
            }
        }
    }
    map
}

fn upsert_config(path: &Path, updates: &[(&str, &str)]) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut updated = std::collections::HashSet::new();
    let mut in_global = false;
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim().to_string();
        if trimmed.starts_with('[') {
            in_global = trimmed.trim_end() == "[global]";
            i += 1;
            continue;
        }
        if in_global
            && !trimmed.starts_with('#')
            && let Some(eq_pos) = trimmed.find('=')
        {
            let key = trimmed[..eq_pos].trim().to_string();
            if let Some(idx) = updates.iter().position(|(k, _)| *k == key.as_str()) {
                lines[i] = format!("{key} = \"{}\"", updates[idx].1);
                updated.insert(key);
            }
        }
        i += 1;
    }

    let has_global = lines.iter().any(|l| l.trim().trim_end() == "[global]");
    if !updates.is_empty() && updated.len() < updates.len() {
        let insert_pos = if has_global {
            lines
                .iter()
                .position(|l| l.trim().trim_end() == "[global]")
                .unwrap()
                + 1
        } else {
            lines.len()
        };
        if !has_global {
            lines.insert(insert_pos, "[global]".into());
        }
        let mut offset = if has_global { 0 } else { 1 };
        for (key, val) in updates {
            if !updated.contains(*key) {
                lines.insert(insert_pos + offset, format!("{key} = \"{val}\""));
                offset += 1;
            }
        }
    } else if updated.is_empty() && !updates.is_empty() {
        lines.push(String::new());
        lines.push("[global]".into());
        for (key, val) in updates {
            lines.push(format!("{key} = \"{val}\""));
        }
    }

    std::fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

pub async fn run_wizard(data_dir: &Path, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut catalog: Vec<(String, Vec<ModelInfo>)> = Vec::new();
    for cat_dir in MANIFESTS.dirs() {
        let cat_name = dir_name(cat_dir).to_string();
        let mut models = Vec::new();
        for file_entry in cat_dir.files() {
            if file_entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Ok(entry) = serde_json::from_slice::<ModelEntry>(file_entry.contents()) else {
                continue;
            };
            let display = file_entry
                .path()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let toml_model = entry
                .config
                .as_ref()
                .map(|c| c.model_name.clone())
                .unwrap_or_else(|| display.clone());
            let default_variant = entry.default_variant.unwrap_or_else(|| "default".into());
            let variants: Vec<String> = entry.variants.keys().cloned().collect();
            models.push(ModelInfo {
                display,
                toml_model,
                default_variant,
                variants,
            });
        }
        if !models.is_empty() {
            catalog.push((cat_name, models));
        }
    }

    // Find config & load existing selections
    let config_path = find_config();
    if let Some(ref p) = config_path {
        if p.exists() {
            println!("\nFound config: {}", p.display());
        } else {
            println!("\nConfig file: {} (will create)", p.display());
        }
    }
    let existing = config_path
        .as_ref()
        .map(|p| load_selections(p))
        .unwrap_or_default();

    // Selection state: category → (manifest_name, toml_model, variant)
    let mut selections: HashMap<String, (String, String, String)> = HashMap::new();
    let cat_names: Vec<String> = catalog.iter().map(|(c, _)| c.clone()).collect();

    // Pre-populate from existing config
    for (key, val) in &existing {
        if let Some(cat) = key.strip_suffix("_model")
            && let Some(model_info) = catalog
                .iter()
                .find(|(c, _)| c == cat)
                .and_then(|(_, models)| models.iter().find(|m| m.toml_model == *val))
        {
            let variant = existing
                .get(&format!("{cat}_variant"))
                .cloned()
                .unwrap_or_else(|| model_info.default_variant.clone());
            selections.insert(
                cat.to_string(),
                (
                    model_info.display.clone(),
                    model_info.toml_model.clone(),
                    variant,
                ),
            );
        }
    }

    // Display catalog
    println!("\nAvailable models:\n");
    for (cat_name, models) in &catalog {
        println!("  {cat_name}:");
        for m in models {
            let selected = selections.contains_key(cat_name);
            let sel = if selected { " [SELECTED]" } else { "" };
            println!("    {} (config model: {}){}", m.display, m.toml_model, sel);
            for v in &m.variants {
                let def = if *v == m.default_variant {
                    " (default)"
                } else {
                    ""
                };
                println!("      - {v}{def}");
            }
        }
    }

    // Selection loop
    let mut cat_items = cat_names.clone();
    cat_items.push("done".into());
    loop {
        let idx = Select::new()
            .with_prompt("Select category")
            .items(&cat_items)
            .interact()?;
        let input = &cat_items[idx];
        if input == "done" {
            break;
        }

        let cat = input.clone();
        let models = &catalog.iter().find(|(c, _)| c == &cat).unwrap().1;
        let model_names: Vec<String> = models.iter().map(|m| m.display.clone()).collect();
        let model_idx = Select::new()
            .with_prompt("Select model")
            .items(&model_names)
            .interact()?;
        let display = &model_names[model_idx];
        let entry = models.iter().find(|m| m.display == *display).unwrap();

        let var = if entry.variants.len() > 1 {
            let old_var = existing.get(&format!("{cat}_variant")).map(|s| s.as_str());
            let default = old_var.unwrap_or(&entry.default_variant);
            let default_idx = entry
                .variants
                .iter()
                .position(|v| v == default)
                .unwrap_or(0);
            let var_idx = Select::new()
                .with_prompt("Select variant")
                .items(&entry.variants)
                .default(default_idx)
                .interact()?;
            entry.variants[var_idx].clone()
        } else {
            entry.default_variant.clone()
        };

        selections.insert(
            cat.clone(),
            (entry.display.clone(), entry.toml_model.clone(), var.clone()),
        );
        println!("  ✓ Added {}/{} ({})", cat, entry.display, var);
    }

    // Final summary
    println!("\n── Selections ──");
    for (cat_name, models) in &catalog {
        if let Some((_, toml_model, var)) = selections.get(cat_name) {
            let path = format!(
                "data/{cat_name}/model/{}/{}",
                models
                    .iter()
                    .find(|m| m.toml_model == *toml_model)
                    .map(|m| m.display.as_str())
                    .unwrap_or(toml_model),
                var
            );
            println!("  {cat_name}:  model={toml_model}  variant={var}  path={path}");
        } else {
            println!("  {cat_name}:  (not selected)");
        }
    }

    // Collect update lines
    let mut updates: Vec<(&str, &str)> = Vec::new();
    // Maintain ordering: tts, asr, llm, vad
    for cat in &["tts", "asr", "llm", "vad"] {
        if let Some((_, toml_model, var)) = selections.get(*cat) {
            let model_entry = catalog
                .iter()
                .find(|(c, _)| c == cat)
                .and_then(|(_, ms)| ms.iter().find(|m| m.toml_model == *toml_model));
            let has_variants = model_entry.map(|m| m.variants.len() > 1).unwrap_or(false);
            let model_key = format!("{cat}_model");
            updates.push((
                Box::leak(model_key.into_boxed_str()),
                Box::leak(toml_model.clone().into_boxed_str()),
            ));
            if has_variants {
                let var_key = format!("{cat}_variant");
                updates.push((
                    Box::leak(var_key.into_boxed_str()),
                    Box::leak(var.clone().into_boxed_str()),
                ));
            }
            let path = format!(
                "data/{cat}/model/{}/{}",
                model_entry
                    .map(|m| m.display.as_str())
                    .unwrap_or(toml_model),
                var
            );
            let path_key = format!("{cat}_path");
            updates.push((
                Box::leak(path_key.into_boxed_str()),
                Box::leak(path.into_boxed_str()),
            ));
        }
    }

    if confirm("\nWrite to config file?")? {
        let path = config_path
            .as_deref()
            .unwrap_or(Path::new("application.toml"));
        upsert_config(path, &updates)?;
        println!("✓ Written to {}", path.display());
    }

    if !selections.is_empty() && confirm("Download all selected models?")? {
        let mir: Vec<String> = Vec::new();
        for (cat, (display, _, var)) in &selections {
            println!("\n--- Downloading {cat}/{display}/{var} ---");
            if let Err(e) = run(
                Some(cat),
                Some(display.as_str()),
                Some(var.as_str()),
                data_dir,
                quiet,
                &mir,
                None,
                None,
                false,
            )
            .await
            {
                eprintln!("  FAIL: {e}");
            }
        }
    }

    Ok(())
}

fn confirm(question: &str) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(dialoguer::Confirm::new()
        .with_prompt(question)
        .default(false)
        .interact()?)
}

fn config_to_targets(config: &AppConfig) -> Vec<(String, String, Option<String>)> {
    let mut targets = Vec::new();

    if let Some(ref model) = config.tts_model
        && let Some((_, _, stem)) = tts_model_info(model)
    {
        targets.push(("tts".into(), stem, config.tts_variant.clone()));
    }

    if let Some(ref model) = config.asr_model
        && let Some((_, _, stem)) = asr_model_info(model)
    {
        targets.push(("asr".into(), stem, config.asr_variant.clone()));
    }

    match config
        .llm_model
        .clone()
        .expect("llm_model should have default")
    {
        LlmModel::Qwen3 => {
            targets.push(("llm".into(), "qwen3".into(), config.llm_variant.clone()));
        }
        LlmModel::Echo => {}
    }

    match config
        .vad_model
        .clone()
        .expect("vad_model should have default")
    {
        VadModel::Earshot | VadModel::Void => {}
    }

    targets
}

pub fn update_checksums(data_dir: &Path, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
    let report_sha: HashMap<String, String> = std::fs::read(data_dir.join("download-report.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Report>(&bytes).ok())
        .map(|r| {
            r.files
                .into_iter()
                .filter(|f| f.status == "ok")
                .map(|f| (f.path, f.sha256))
                .collect()
        })
        .unwrap_or_default();

    let mut sha_updates: Vec<(PathBuf, String, String)> = Vec::new();

    for cat_dir in MANIFESTS.dirs() {
        let _cat_name = dir_name(cat_dir);
        for file_entry in cat_dir.files() {
            if file_entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }

            let _model_name = file_entry.path().file_stem().unwrap().to_str().unwrap();
            let entry: ModelEntry = serde_json::from_slice(file_entry.contents())?;
            let variants = resolve_variants(&entry, None);
            let manifest_rel = file_entry.path().to_path_buf();

            for v in variants.values() {
                for file in &v.files {
                    let dest = data_dir.join(&file.path);
                    if !dest.exists() {
                        if !quiet {
                            eprintln!("  SKIP (not found): {}", dest.display());
                        }
                        continue;
                    }

                    let sha = match report_sha.get(&file.path) {
                        Some(s) => s.clone(),
                        None => {
                            let (_size, sha) = sha256_file(&dest)?;
                            sha
                        }
                    };

                    if !quiet {
                        eprintln!("  {:<60} {}", dest.display(), sha);
                    }
                    sha_updates.push((manifest_rel.clone(), file.path.clone(), sha));
                }

                for archive in &v.archives {
                    // Archive itself: compute sha256 from disk (archive is kept after extraction)
                    let dest_dir = data_dir.join(&archive.path);
                    let archive_file = dest_dir.with_file_name(format!(
                        "{}.tar.bz2",
                        dest_dir
                            .file_name()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or("model")
                    ));
                    if archive_file.exists() {
                        let (_size, sha) = sha256_file(&archive_file)?;
                        if !quiet {
                            eprintln!("  {:<60} {}", archive_file.display(), sha);
                        }
                        sha_updates.push((manifest_rel.clone(), archive.path.clone(), sha));
                    } else {
                        // fallback: look up from download report (try new and old key format)
                        let report_key = archive_report_path(archive);
                        let old_key = format!("{} (archive)", archive.path);
                        if let Some(sha) = report_sha
                            .get(&report_key)
                            .or_else(|| report_sha.get(&old_key))
                        {
                            sha_updates.push((
                                manifest_rel.clone(),
                                archive.path.clone(),
                                sha.clone(),
                            ));
                        }
                    }

                    // Extracted files: compute sha256 from disk
                    for entry in &archive.extract {
                        let file_path = extract_target_path(&dest_dir, entry);
                        if !file_path.exists() || file_path.is_dir() {
                            if !quiet && !file_path.exists() {
                                eprintln!("  SKIP (not found): {}", file_path.display());
                            }
                            continue;
                        }
                        let (_size, sha) = sha256_file(&file_path)?;
                        if !quiet {
                            eprintln!("  {:<60} {}", file_path.display(), sha);
                        }
                        sha_updates.push((manifest_rel.clone(), entry.path.clone(), sha));
                    }
                }
            }
        }
    }

    if sha_updates.is_empty() {
        if !quiet {
            eprintln!("No downloaded files found in {}", data_dir.display());
        }
        return Ok(());
    }

    write_checksums_to_manifests(&sha_updates)?;
    if !quiet {
        eprintln!("Updated {} file(s) in manifests", sha_updates.len());
    }

    Ok(())
}

/// Look up TTS model info from embedded manifest by serde model name.
/// Returns (default_variant, base_path, manifest_file_stem).
fn tts_model_info(model: &TtsModel) -> Option<(String, String, String)> {
    let model_str = serde_json::to_value(model).ok()?.as_str()?.to_owned();
    let cat_dir = MANIFESTS.get_dir("tts")?;
    for file_entry in cat_dir.files() {
        let entry: serde_json::Value = serde_json::from_slice(file_entry.contents()).ok()?;
        if entry["config"]["model"].as_str() != Some(&model_str) {
            continue;
        }
        let stem = file_entry.path().file_stem()?.to_str()?.to_owned();
        let default_variant = entry["default_variant"].as_str()?.to_owned();
        let archive_path = entry["variants"][&default_variant]["archives"][0]["path"].as_str()?;
        let base = Path::new(archive_path).parent()?.to_str()?.to_owned();
        let base_path = format!("{base}/");
        return Some((default_variant, base_path, stem));
    }
    None
}

/// Returns the default variant name for a given TTS model from its embedded manifest.
pub fn default_tts_variant(model: &TtsModel) -> Option<String> {
    tts_model_info(model).map(|(v, _, _)| v)
}

/// Returns the base storage path for a TTS model (e.g. "tts/model/matcha/").
/// Derived from the default variant's archive path in the manifest.
pub fn tts_base_path(model: &TtsModel) -> Option<String> {
    tts_model_info(model).map(|(_, b, _)| b)
}

/// Returns the default `length_scale` for the given model+variant from the manifest.
pub fn tts_length_scale(model: &TtsModel, variant: &str) -> Option<f32> {
    let (_, _, stem) = tts_model_info(model)?;
    let cat_dir = MANIFESTS.get_dir("tts")?;
    let file_entry = cat_dir
        .files()
        .find(|f| f.path().file_stem() == Some(OsStr::new(&stem)))?;
    let entry: serde_json::Value = serde_json::from_slice(file_entry.contents()).ok()?;
    entry["variants"][variant]["length_scale"]
        .as_f64()
        .map(|v| v as f32)
}

/// Look up ASR model info from embedded manifest by serde model name.
/// Returns (default_variant, base_path, manifest_file_stem).
fn asr_model_info(model: &AsrModel) -> Option<(String, String, String)> {
    let model_str = serde_json::to_value(model).ok()?.as_str()?.to_owned();
    let cat_dir = MANIFESTS.get_dir("asr")?;
    for file_entry in cat_dir.files() {
        let entry: serde_json::Value = serde_json::from_slice(file_entry.contents()).ok()?;
        if entry["config"]["model"].as_str() != Some(&model_str) {
            continue;
        }
        let stem = file_entry.path().file_stem()?.to_str()?.to_owned();
        let default_variant = entry["default_variant"].as_str()?.to_owned();
        let archive_path = entry["variants"][&default_variant]["archives"][0]["path"].as_str()?;
        let base = std::path::Path::new(archive_path)
            .parent()?
            .to_str()?
            .to_owned();
        let base_path = format!("{base}/");
        return Some((default_variant, base_path, stem));
    }
    None
}

/// Returns the default variant name for a given ASR model from its embedded manifest.
pub fn default_asr_variant(model: &AsrModel) -> Option<String> {
    asr_model_info(model).map(|(v, _, _)| v)
}

/// Returns the base storage path for an ASR model (e.g. "asr/model/sense_voice/").
/// Derived from the default variant's archive path in the manifest.
pub fn asr_base_path(model: &AsrModel) -> Option<String> {
    asr_model_info(model).map(|(_, b, _)| b)
}

/// Returns the default reference audio variant from the embedded manifest.
pub fn default_reference_variant() -> Option<String> {
    let cat_dir = MANIFESTS.get_dir("reference")?;
    let file_entry = cat_dir
        .files()
        .find(|f| f.path().file_stem() == Some(OsStr::new("audio")))?;
    let entry: serde_json::Value = serde_json::from_slice(file_entry.contents()).ok()?;
    entry["default_variant"].as_str().map(String::from)
}

/// Look up reference audio path and prompt text from the embedded manifest.
pub fn resolve_reference_audio(variant: &str) -> Option<(String, String)> {
    let cat_dir = MANIFESTS.get_dir("reference")?;
    let file_entry = cat_dir
        .files()
        .find(|f| f.path().file_stem() == Some(OsStr::new("audio")))?;
    let entry: serde_json::Value = serde_json::from_slice(file_entry.contents()).ok()?;
    let variant_obj = entry["variants"].get(variant)?;
    let path = variant_obj["files"][0]["path"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            let ap = variant_obj["archives"][0]["path"].as_str()?;
            let ex = variant_obj["archives"][0]["extract"][0]["path"].as_str()?;
            let ex_filename = Path::new(ex).file_name().and_then(|f| f.to_str())?;
            Some(format!("{ap}{ex_filename}"))
        })?;
    let prompt_text = variant_obj["prompt_text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    Some((path, prompt_text))
}

#[cfg(test)]
mod tests;
