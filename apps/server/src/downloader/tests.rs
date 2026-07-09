use super::*;
use std::fs;

// ── helpers ──

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("chobits-test").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha256_of(data: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn make_file(path: &str, url: &str) -> FileEntry {
    FileEntry {
        path: path.into(),
        url: url.into(),
        sha256: None,
    }
}

fn make_entry(files: Vec<(&str, Vec<FileEntry>)>, default_variant: Option<&str>) -> ModelEntry {
    ModelEntry {
        config: None,
        default_variant: default_variant.map(|s| s.into()),
        variants: files
            .into_iter()
            .map(|(k, v)| {
                (
                    k.into(),
                    Variant {
                        files: v,
                        archives: vec![],
                        prompt_text: None,
                    },
                )
            })
            .collect(),
    }
}

// ── generate_urls ──

#[test]
fn test_generate_urls_hf_adds_default_mirror() {
    let urls = generate_urls("https://huggingface.co/model/file.bin", &[]);
    assert_eq!(urls.len(), 2);
    assert_eq!(urls[0], "https://huggingface.co/model/file.bin");
    assert_eq!(urls[1], "https://hf-mirror.com/model/file.bin");
}

#[test]
fn test_generate_urls_non_hf_no_mirror() {
    let urls = generate_urls("https://example.com/model.bin", &[]);
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0], "https://example.com/model.bin");
}

#[test]
fn test_generate_urls_custom_mirrors() {
    let urls = generate_urls(
        "https://huggingface.co/model.bin",
        &["https://my-mirror.com".into()],
    );
    assert_eq!(urls.len(), 2);
    assert_eq!(urls[1], "https://my-mirror.com/model.bin");
}

#[test]
fn test_generate_urls_empty_mirror_vec() {
    let urls = generate_urls("https://huggingface.co/model.bin", &Vec::new());
    assert_eq!(urls.len(), 2);
    assert_eq!(urls[1], "https://hf-mirror.com/model.bin");
}

// ── resolve_variants ──

#[test]
fn test_resolve_variants_specific() {
    let entry = make_entry(
        vec![
            ("a", vec![make_file("f1", "u1")]),
            ("b", vec![make_file("f2", "u2")]),
        ],
        Some("a"),
    );
    let r = resolve_variants(&entry, Some("b"));
    assert_eq!(r.len(), 1);
    assert!(r.contains_key("b"));
}

#[test]
fn test_resolve_variants_default() {
    let entry = make_entry(
        vec![
            ("a", vec![make_file("f1", "u1")]),
            ("b", vec![make_file("f2", "u2")]),
        ],
        Some("a"),
    );
    let r = resolve_variants(&entry, None);
    assert_eq!(r.len(), 1);
    assert!(r.contains_key("a"));
}

#[test]
fn test_resolve_variants_no_default_gives_all() {
    let entry = make_entry(
        vec![
            ("a", vec![make_file("f1", "u1")]),
            ("b", vec![make_file("f2", "u2")]),
        ],
        None,
    );
    let r = resolve_variants(&entry, None);
    assert_eq!(r.len(), 2);
}

#[test]
fn test_resolve_variants_unknown_empty() {
    let entry = make_entry(vec![("a", vec![make_file("f1", "u1")])], Some("a"));
    let r = resolve_variants(&entry, Some("unknown"));
    assert!(r.is_empty());
}

// ── config_to_targets ──

fn make_cfg(v: serde_json::Value) -> api::config::Config {
    serde_json::from_value(v).unwrap()
}

#[test]
fn test_config_to_targets_defaults() {
    let t = config_to_targets(&make_cfg(serde_json::json!({})));
    // defaults: tts=MatchaTts, asr=SenseVoice, llm=Qwen3, vad=Earshot
    assert_eq!(t.len(), 3);
    assert!(t.contains(&("tts".into(), "matcha".into(), None)));
    assert!(t.contains(&("asr".into(), "sense_voice".into(), None)));
    assert!(t.contains(&("llm".into(), "qwen3".into(), None)));
}

#[test]
fn test_config_to_targets_all_quiet() {
    let t = config_to_targets(&make_cfg(serde_json::json!({
        "tts_model": "mute",
        "asr_model": "void",
        "llm_model": "echo",
        "vad_model": "void",
    })));
    assert!(t.is_empty());
}

#[test]
fn test_config_to_targets_mute() {
    let t = config_to_targets(&make_cfg(serde_json::json!({
        "tts_model": "mute",
        "asr_model": "void",
        "llm_model": "echo",
        "vad_model": "void",
    })));
    assert!(t.is_empty());
}

// ── sha256_file ──

#[test]
fn test_sha256_file_known() {
    let dir = test_dir("sha256_file");
    let path = dir.join("data.bin");
    let data = b"hello world";
    fs::write(&path, data).unwrap();
    let (size, sha) = sha256_file(&path).unwrap();
    assert_eq!(size, data.len() as u64);
    assert_eq!(sha, sha256_of(data));
}

// ── set_sha256 ──

#[test]
fn test_set_sha256_flat() {
    let mut v = serde_json::json!({"path": "m.bin", "url": "http://x"});
    set_sha256(&mut v, "m.bin", "abc");
    assert_eq!(v["sha256"], "abc");
}

#[test]
fn test_set_sha256_nested() {
    let mut v = serde_json::json!({
        "variants": {
            "d": {
                "files": [
                    {"path": "a", "url": "u1"},
                    {"path": "b", "url": "u2"},
                ]
            }
        }
    });
    set_sha256(&mut v, "b", "s1");
    assert_eq!(v["variants"]["d"]["files"][1]["sha256"], "s1");
    assert!(v["variants"]["d"]["files"][0].get("sha256").is_none());
}

#[test]
fn test_set_sha256_no_match() {
    let mut v = serde_json::json!({"files": [{"path": "a", "url": "u"}]});
    set_sha256(&mut v, "unknown", "x");
    assert!(v["files"][0].get("sha256").is_none());
}

// ── load_selections ──

#[test]
fn test_load_selections_valid() {
    let dir = test_dir("load_sel");
    let p = dir.join("c.toml");
    fs::write(&p, "[global]\ntts_model = \"mute\"\n").unwrap();
    let m = load_selections(&p);
    assert_eq!(m.get("tts_model").unwrap(), "mute");
}

#[test]
fn test_load_selections_no_global() {
    let dir = test_dir("load_sel_nog");
    let p = dir.join("c.toml");
    fs::write(&p, "[other]\nk = \"v\"\n").unwrap();
    assert!(load_selections(&p).is_empty());
}

#[test]
fn test_load_selections_empty() {
    let dir = test_dir("load_sel_emp");
    let p = dir.join("c.toml");
    fs::write(&p, "").unwrap();
    assert!(load_selections(&p).is_empty());
}

// ── upsert_config ──

#[test]
fn test_upsert_config_new() {
    let dir = test_dir("upsert_new");
    let p = dir.join("c.toml");
    upsert_config(&p, &[("tts_model", "mute")]).unwrap();
    let c = fs::read_to_string(&p).unwrap();
    assert!(c.contains("tts_model = \"mute\""));
}

#[test]
fn test_upsert_config_update() {
    let dir = test_dir("upsert_upd");
    let p = dir.join("c.toml");
    fs::write(&p, "[global]\ntts_model = \"old\"\n").unwrap();
    upsert_config(&p, &[("tts_model", "new"), ("tts_variant", "1.5b")]).unwrap();
    let c = fs::read_to_string(&p).unwrap();
    assert!(c.contains("tts_model = \"new\""));
    assert!(c.contains("tts_variant = \"1.5b\""));
}

#[test]
fn test_upsert_config_preserve_sections() {
    let dir = test_dir("upsert_pres");
    let p = dir.join("c.toml");
    fs::write(&p, "[global]\ntts_model = \"a\"\n[other]\nk = \"v\"\n").unwrap();
    upsert_config(&p, &[("tts_model", "b")]).unwrap();
    let c = fs::read_to_string(&p).unwrap();
    assert!(c.contains("[other]"));
    assert!(c.contains("k = \"v\""));
    assert!(c.contains("tts_model = \"b\""));
}

#[test]
fn test_upsert_config_add_global() {
    let dir = test_dir("upsert_add_g");
    let p = dir.join("c.toml");
    fs::write(&p, "[other]\nk = \"v\"\n").unwrap();
    upsert_config(&p, &[("tts_model", "m")]).unwrap();
    let c = fs::read_to_string(&p).unwrap();
    assert!(c.contains("[global]"));
    assert!(c.contains("tts_model = \"m\""));
}

// ── find_config ──

fn with_cwd<F>(dir: &Path, f: F)
where
    F: FnOnce(),
{
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    f();
    std::env::set_current_dir(&orig).unwrap();
}

#[test]
fn test_find_config_env_var() {
    let dir = test_dir("fc_env");
    let p = dir.join("my.toml");
    fs::write(&p, "").unwrap();
    let r = find_config_inner(Some(p.to_str().unwrap().into()));
    assert_eq!(r, Some(p));
}

#[test]
fn test_find_config_finds_application_toml() {
    let dir = test_dir("fc_app");
    with_cwd(&dir, || {
        fs::write("application.toml", "").unwrap();
        assert_eq!(
            find_config_inner(None),
            Some(PathBuf::from("application.toml"))
        );
    });
}

#[test]
fn test_find_config_fallback() {
    let dir = test_dir("fc_fb");
    with_cwd(&dir, || {
        assert_eq!(
            find_config_inner(None),
            Some(PathBuf::from("application.toml"))
        );
    });
}

// ── write_checksums_to_manifests ──

#[test]
fn test_write_checksums_integration() {
    let base = Path::new(MANIFESTS_DIR);
    if !base.exists() {
        eprintln!("  SKIP: MANIFESTS_DIR ({}) does not exist", base.display());
        return;
    }

    let rel = PathBuf::from("_test_write_checksums.json");
    let full = base.join(&rel);
    let _ = std::fs::remove_file(&full);

    let original = serde_json::json!({
        "files": [
            {"path": "data/test.bin", "url": "http://example.com/test.bin", "sha256": null}
        ]
    });
    std::fs::write(&full, serde_json::to_string_pretty(&original).unwrap()).unwrap();

    let updates = vec![(rel.clone(), "data/test.bin".into(), "test_sha_value".into())];

    let result = write_checksums_to_manifests(&updates);
    assert!(
        result.is_ok(),
        "write_checksums_to_manifests failed: {result:?}"
    );

    let updated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&full).unwrap()).unwrap();
    assert_eq!(updated["files"][0]["sha256"], "test_sha_value");

    let _ = std::fs::remove_file(&full);
}

// ── update_checksums ──

#[test]
fn test_update_checksums_updates_manifest() {
    let base = Path::new(MANIFESTS_DIR);
    if !base.exists() {
        eprintln!("  SKIP: MANIFESTS_DIR ({}) does not exist", base.display());
        return;
    }

    // Use a real embedded manifest: reference/audio.json has one file entry
    let manifest_rel = PathBuf::from("reference/audio.json");
    let manifest_path = base.join(&manifest_rel);
    let original_content = std::fs::read_to_string(&manifest_path).unwrap();

    let data_dir = test_dir("update_cksum");

    // The manifest has file.path = "tts/reference/xiyangyang.wav"
    let file_rel = "tts/reference/xiyangyang.wav";
    let content = b"test content for update_checksums";
    let abs_file = data_dir.join(file_rel);
    std::fs::create_dir_all(abs_file.parent().unwrap()).unwrap();
    std::fs::write(&abs_file, content).unwrap();

    let result = update_checksums(&data_dir, true);
    assert!(result.is_ok(), "update_checksums failed: {result:?}");

    // Verify the real manifest was updated
    let updated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let expected_sha = sha256_of(content);
    assert_eq!(
        updated["variants"]["xiyangyang"]["files"][0]["sha256"], expected_sha,
        "SHA256 was not written correctly"
    );

    // Restore original manifest content
    std::fs::write(&manifest_path, &original_content).unwrap();
}

// ── load_overrides ──

#[tokio::test]
async fn test_load_overrides_local() {
    let dir = test_dir("lo");
    let p = dir.join("ov.json");
    fs::write(
        &p,
        r#"{"data/m.bin": {"url": "http://localhost/m.bin", "sha256": "ab"}}"#,
    )
    .unwrap();
    let m = load_overrides(p.to_str().unwrap()).await.unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m["data/m.bin"].url, "http://localhost/m.bin");
    assert_eq!(m["data/m.bin"].sha256.as_deref(), Some("ab"));
}

// ── try_download_url (mockito) ──

#[tokio::test]
async fn test_try_download_ok() {
    let mut srv = mockito::Server::new_async().await;
    let body = b"hello";
    let m = srv
        .mock("GET", "/f.bin")
        .with_status(200)
        .with_body(body)
        .expect(1)
        .create();

    let dir = test_dir("tdl_ok");
    let dest = dir.join("f.bin");
    let url = format!("{}/f.bin", srv.url());

    let r = try_download_url(&Client::new(), &url, &dest, None, true).await;
    assert!(r.is_ok());
    let (sz, sha) = r.unwrap();
    assert_eq!(sz, body.len() as u64);
    assert_eq!(sha, sha256_of(body));
    assert!(dest.exists());
    m.assert();
}

#[tokio::test]
async fn test_try_download_sha_mismatch() {
    let mut srv = mockito::Server::new_async().await;
    let m = srv
        .mock("GET", "/f.bin")
        .with_status(200)
        .with_body(b"hello")
        .expect(1)
        .create();

    let dir = test_dir("tdl_mismatch");
    let dest = dir.join("f.bin");
    let url = format!("{}/f.bin", srv.url());

    let r = try_download_url(
        &Client::new(),
        &url,
        &dest,
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
        true,
    )
    .await;
    assert!(r.is_err());
    assert!(!dest.exists());
    let tmp = PathBuf::from(format!("{}.tmp.{}", dest.display(), std::process::id()));
    assert!(!tmp.exists());
    m.assert();
}

#[tokio::test]
async fn test_try_download_conn_refused() {
    let client = Client::builder().no_proxy().build().unwrap();
    let dir = test_dir("tdl_conn");
    let r = try_download_url(
        &client,
        "http://127.0.0.1:18634/f.bin",
        &dir.join("f.bin"),
        None,
        true,
    )
    .await;
    assert!(r.is_err(), "expected error, got {:?}", r);
}

// ── download_file (mockito) ──

#[tokio::test]
async fn test_download_cached() {
    let mut srv = mockito::Server::new_async().await;
    let body = b"cached";
    let m = srv
        .mock("GET", "/f.bin")
        .with_status(200)
        .with_body(body)
        .expect(0)
        .create();

    let dir = test_dir("dl_cached");
    let dest = dir.join("f.bin");
    let sha = sha256_of(body);
    fs::write(&dest, body).unwrap();

    let r = download_file(
        &Client::new(),
        &format!("{}/f.bin", srv.url()),
        &dest,
        Some(&sha),
        &[],
        true,
    )
    .await;
    assert!(r.is_ok());
    m.assert();
}

#[tokio::test]
async fn test_download_re_download_on_sha_mismatch() {
    let mut srv = mockito::Server::new_async().await;
    let correct = b"correct";
    let m = srv
        .mock("GET", "/f.bin")
        .with_status(200)
        .with_body(correct)
        .expect(1)
        .create();

    let dir = test_dir("dl_redl");
    let dest = dir.join("f.bin");
    let sha = sha256_of(correct);
    fs::write(&dest, b"wrong").unwrap();

    let r = download_file(
        &Client::new(),
        &format!("{}/f.bin", srv.url()),
        &dest,
        Some(&sha),
        &[],
        true,
    )
    .await;
    assert!(r.is_ok());
    assert_eq!(fs::read(&dest).unwrap(), correct);
    m.assert();
}

#[tokio::test]
async fn test_download_fresh() {
    let mut srv = mockito::Server::new_async().await;
    let body = b"fresh";
    let m = srv
        .mock("GET", "/f.bin")
        .with_status(200)
        .with_body(body)
        .expect(1)
        .create();

    let dir = test_dir("dl_fresh");
    let dest = dir.join("f.bin");

    let r = download_file(
        &Client::new(),
        &format!("{}/f.bin", srv.url()),
        &dest,
        None,
        &[],
        true,
    )
    .await;
    assert!(r.is_ok());
    assert_eq!(fs::read(&dest).unwrap(), body);
    m.assert();
}
