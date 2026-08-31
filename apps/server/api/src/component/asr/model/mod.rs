pub mod void;
pub mod x_asr;

pub(crate) fn auto_discover_onnx(dir: &str, prefix: &str) -> Option<String> {
    let p = std::path::Path::new(dir);
    std::fs::read_dir(p).ok().and_then(|mut entries| {
        entries.find_map(|entry| {
            entry.ok().and_then(|e| {
                let path = e.path();
                if path.extension().is_some_and(|ext| ext == "onnx")
                    && path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|stem| stem.contains(prefix))
                {
                    path.to_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
    })
}
