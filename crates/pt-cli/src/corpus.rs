//! License ledger and downloader for the reference corpus (PRO_QUALITY_PLAN section B4 step 1).
//!
//! * `corpus/manifest.json` (tracked) lists every asset, where it comes from and what licence it is under.
//! * Raw files live OUTSIDE the repo in `$PT_CORPUS` (default `D:\Fekri\corpus`), one directory per source.
//! * `corpus/LEDGER.csv` (tracked) has one row per file: id, source, url, license, license_url, sha256, fetched_at,
//!   attribution. `id` is the file's path relative to the corpus root.
//! * `pt corpus fetch` downloads with the system `curl` (sequentially, never running anything it downloads), reads
//!   zip members in memory (only the named member is written, under a name we choose), hashes and records them.
//! * `pt corpus verify` re-hashes every ledger row and refuses anything that is not CC0 or US-government public domain (`PD-USGS`).

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LEDGER_COLUMNS: [&str; 8] = ["id", "source", "url", "license", "license_url", "sha256", "fetched_at", "attribution"];
/// Licences accepted: CC0 (a public-domain dedication) and `PD-USGS` (data released by the US Geological Survey into the public domain:
/// "All 3DEP products available through The National Map are in the public domain and may be used without restriction"; credit requested).
/// Tests that point `PT_CORPUS` at a temporary directory hold this lock so they cannot race.
#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub const ALLOWED_LICENSES: [&str; 2] = ["CC0-1.0", "PD-USGS"];

pub fn root() -> PathBuf {
    if let Ok(p) = std::env::var("PT_CORPUS") {
        return PathBuf::from(p);
    }
    let d = PathBuf::from(r"D:\Fekri\corpus");
    if d.exists() { d } else { PathBuf::from("corpus/raw") }
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    /// Path relative to the corpus root (also the ledger id).
    pub path: String,
    pub url: Option<String>,
    pub archive: Option<String>,
    pub archive_url: Option<String>,
    pub member: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AssetEntry {
    pub id: String,
    pub source: String,
    pub set: String,
    pub category: String,
    pub difficulty: String,
    pub license: String,
    pub license_url: String,
    pub attribution: String,
    /// Path (relative to the root) of the file the importer reads.
    pub main: String,
    pub files: Vec<FileEntry>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn so(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(String::from)
}

pub fn load_manifest(path: &Path) -> Result<Vec<AssetEntry>, String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for a in v.get("assets").and_then(Value::as_array).ok_or("manifest needs an 'assets' array")? {
        let files = a
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("asset {} has no files", s(a, "id")))?
            .iter()
            .map(|f| FileEntry { path: s(f, "path"), url: so(f, "url"), archive: so(f, "archive"), archive_url: so(f, "archive_url"), member: so(f, "member") })
            .collect();
        out.push(AssetEntry {
            id: s(a, "id"),
            source: s(a, "source"),
            set: s(a, "set"),
            category: s(a, "category"),
            difficulty: s(a, "difficulty"),
            license: s(a, "license"),
            license_url: s(a, "license_url"),
            attribution: s(a, "attribution"),
            main: s(a, "main"),
            files,
        });
    }
    Ok(out)
}

// ---- CSV (RFC 4180 subset) ----

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv_parse(text: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cur, mut quoted) = (Vec::new(), Vec::new(), String::new(), false);
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match (c, quoted) {
            ('"', true) if it.peek() == Some(&'"') => {
                cur.push('"');
                it.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => row.push(std::mem::take(&mut cur)),
            ('\n', false) | ('\r', false) => {
                if c == '\r' && it.peek() == Some(&'\n') {
                    it.next();
                }
                if !cur.is_empty() || !row.is_empty() {
                    row.push(std::mem::take(&mut cur));
                    rows.push(std::mem::take(&mut row));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() || !row.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    rows
}

#[derive(Clone, Debug, Default)]
pub struct Ledger {
    pub rows: BTreeMap<String, Vec<String>>,
}

impl Ledger {
    pub fn load(path: &Path) -> Result<Ledger, String> {
        let mut l = Ledger::default();
        let Ok(text) = std::fs::read_to_string(path) else { return Ok(l) };
        for (i, r) in csv_parse(&text).into_iter().enumerate() {
            if i == 0 {
                if r != LEDGER_COLUMNS {
                    return Err(format!("{}: unexpected header {r:?}", path.display()));
                }
                continue;
            }
            if r.len() != LEDGER_COLUMNS.len() {
                return Err(format!("{}: row {} has {} fields", path.display(), i + 1, r.len()));
            }
            l.rows.insert(r[0].clone(), r);
        }
        Ok(l)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut text = LEDGER_COLUMNS.join(",");
        text.push('\n');
        for r in self.rows.values() {
            text.push_str(&r.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
            text.push('\n');
        }
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Today's UTC date and time as ISO-8601 (civil-from-days, no time-zone database needed).
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn curl(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = dest.with_extension("part");
    let st = Command::new("curl")
        .args(["-sS", "-L", "--fail", "--retry", "2", "-A", "proteus-corpus/0.1 (research; contact via repo owner)", "-o"])
        .arg(&tmp)
        .arg(url)
        .status()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !st.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("download failed: {url}"));
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// Read one member of a zip into memory. Only the exact member asked for is read; names inside the archive are
/// never used as output paths.
fn zip_member(archive: &Path, member: &str) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut m = z.by_name(member).map_err(|_| format!("{}: no member '{member}'", archive.display()))?;
    if m.size() > 512 * 1024 * 1024 {
        return Err(format!("member '{member}' is suspiciously large"));
    }
    let mut buf = Vec::with_capacity(m.size() as usize);
    m.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

pub fn fetch(manifest: &Path, ledger_path: &Path, only_set: Option<&str>) -> Result<(), String> {
    let root = root();
    let assets = load_manifest(manifest)?;
    let mut ledger = Ledger::load(ledger_path)?;
    let (mut downloaded, mut kept) = (0, 0);
    for a in &assets {
        if only_set.is_some_and(|s| s != a.set) {
            continue;
        }
        if !ALLOWED_LICENSES.contains(&a.license.as_str()) {
            return Err(format!("{}: licence '{}' is not allowed in the corpus (only {ALLOWED_LICENSES:?})", a.id, a.license));
        }
        for f in &a.files {
            let dest = root.join(&f.path);
            let source_url;
            if !dest.exists() {
                match (&f.url, &f.archive, &f.member) {
                    (Some(u), _, _) => {
                        curl(u, &dest)?;
                        source_url = u.clone();
                    }
                    (None, Some(arch), Some(mem)) => {
                        let arch_path = root.join(&a.source).join("_archives").join(arch);
                        let aurl = f.archive_url.clone().ok_or_else(|| format!("{}: archive_url missing", f.path))?;
                        if !arch_path.exists() {
                            curl(&aurl, &arch_path)?;
                        }
                        let bytes = zip_member(&arch_path, mem)?;
                        if let Some(dir) = dest.parent() {
                            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
                        source_url = format!("{aurl}#{mem}");
                    }
                    _ => return Err(format!("{}: needs 'url' or 'archive'+'member'", f.path)),
                }
                downloaded += 1;
            } else {
                kept += 1;
                source_url = match (&f.url, &f.archive_url, &f.member) {
                    (Some(u), _, _) => u.clone(),
                    (None, Some(au), Some(m)) => format!("{au}#{m}"),
                    _ => String::new(),
                };
            }
            let bytes = std::fs::read(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            let row = vec![f.path.clone(), a.source.clone(), source_url, a.license.clone(), a.license_url.clone(), sha256_hex(&bytes), now_iso(), a.attribution.clone()];
            // Keep the original fetch time when the hash did not change.
            match ledger.rows.get(&f.path) {
                Some(old) if old[5] == row[5] => {}
                _ => {
                    ledger.rows.insert(f.path.clone(), row);
                }
            }
        }
    }
    ledger.save(ledger_path)?;
    println!("corpus: {downloaded} downloaded, {kept} already present, {} ledger rows", ledger.rows.len());
    Ok(())
}

pub struct VerifyReport {
    pub rows: usize,
    pub problems: Vec<String>,
}

pub fn verify(manifest: &Path, ledger_path: &Path) -> Result<VerifyReport, String> {
    let root = root();
    let ledger = Ledger::load(ledger_path)?;
    let mut problems = Vec::new();
    for (id, r) in &ledger.rows {
        if !ALLOWED_LICENSES.contains(&r[3].as_str()) {
            problems.push(format!("{id}: licence '{}' is not allowed (CC0-1.0 or PD-USGS only)", r[3]));
        }
        match std::fs::read(root.join(id)) {
            Ok(b) if sha256_hex(&b) == r[5] => {}
            Ok(_) => problems.push(format!("{id}: hash differs from the ledger")),
            Err(e) => problems.push(format!("{id}: {e}")),
        }
    }
    for a in load_manifest(manifest)? {
        for f in &a.files {
            if !ledger.rows.contains_key(&f.path) {
                problems.push(format!("{}: in the manifest but not in the ledger", f.path));
            }
        }
    }
    Ok(VerifyReport { rows: ledger.rows.len(), problems })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_round_trips_awkward_fields() {
        let rows = vec![vec!["a,b".to_string(), "say \"hi\"".to_string(), "plain".to_string()]];
        let text = rows[0].iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",") + "\n";
        assert_eq!(csv_parse(&text), rows);
    }

    #[test]
    fn sha256_known_answer() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn iso_time_has_the_right_shape() {
        let t = now_iso();
        assert_eq!(t.len(), 20);
        assert!(t.starts_with("20") && t.ends_with('Z'));
    }

    #[test]
    fn ledger_save_load_and_verify_catch_tampering() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("pt_ledger_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.bin"), b"data").unwrap();
        let manifest = dir.join("m.json");
        std::fs::write(&manifest, r#"{"assets":[{"id":"x","source":"src","license":"CC0-1.0","files":[{"path":"src/a.bin","url":"http://unused"}]}]}"#).unwrap();
        let lp = dir.join("L.csv");
        let mut l = Ledger::default();
        l.rows.insert("src/a.bin".into(), vec!["src/a.bin".into(), "src".into(), "u".into(), "CC0-1.0".into(), "lu".into(), sha256_hex(b"data"), "t".into(), "a".into()]);
        l.save(&lp).unwrap();
        std::env::set_var("PT_CORPUS", &dir);
        assert!(verify(&manifest, &lp).unwrap().problems.is_empty());
        std::fs::write(dir.join("src/a.bin"), b"tampered").unwrap();
        assert!(verify(&manifest, &lp).unwrap().problems.iter().any(|p| p.contains("hash differs")));
        let mut l = Ledger::load(&lp).unwrap();
        l.rows.get_mut("src/a.bin").unwrap()[3] = "CC-BY-4.0".into();
        l.save(&lp).unwrap();
        assert!(verify(&manifest, &lp).unwrap().problems.iter().any(|p| p.contains("not allowed")));
        // US public-domain elevation data is accepted next to CC0
        let mut l = Ledger::load(&lp).unwrap();
        l.rows.get_mut("src/a.bin").unwrap()[3] = "PD-USGS".into();
        l.rows.get_mut("src/a.bin").unwrap()[5] = sha256_hex(b"tampered");
        l.save(&lp).unwrap();
        assert!(verify(&manifest, &lp).unwrap().problems.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
