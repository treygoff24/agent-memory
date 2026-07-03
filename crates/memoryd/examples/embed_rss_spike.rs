use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use memory_substrate::EmbeddingTriple;
use memoryd::embedding::{EmbeddingProvider, FastembedProvider};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = std::env::var_os("MEMORUM_SPIKE_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("memorum-embed-rss-spike"));
    std::fs::create_dir_all(&runtime)?;
    if std::env::var_os("HF_HOME").is_none() {
        std::env::set_var("HF_HOME", dirs_home_cache()?);
        println!("hf_home=ambient");
    } else {
        println!("hf_home=preconfigured");
    }

    sample("start")?;
    let triple = EmbeddingTriple {
        provider: memoryd::embedding::FASTEMBED_CANDLE_PROVIDER.to_owned(),
        model_ref: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_MODEL_REF.to_owned(),
        dimension: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_DIMENSION,
    };
    let provider = FastembedProvider::load_for_runtime(&runtime, triple)?;
    sample("provider-loaded")?;
    let docs = [
        "release checklist",
        "kitchen snacks",
        "database engine",
        "policy review",
        "launch daemon",
        "vector recall",
        "governance marker",
        "memory substrate",
    ];
    let _doc_vectors = provider.embed_documents(&docs)?;
    let _query_vector = provider.embed_query("release database recall")?;
    sample("loaded")?;
    drop(provider);
    std::thread::sleep(Duration::from_secs(10));
    sample("post-drop")?;
    std::thread::sleep(Duration::from_secs(60));
    sample("post-drop+60s")?;
    Ok(())
}

fn dirs_home_cache() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cache/huggingface"))
}

fn sample(label: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pid = std::process::id();
    let rss_kib = rss_kib(pid)?;
    let phys_bytes = phys_footprint_bytes(pid)?;
    println!(
        "{label}: rss={} bytes ({:.1} MiB), phys_footprint={} bytes ({:.1} MiB)",
        rss_kib * 1024,
        rss_kib as f64 / 1024.0,
        phys_bytes,
        phys_bytes as f64 / 1024.0 / 1024.0
    );
    Ok(())
}

fn rss_kib(pid: u32) -> Result<u64, Box<dyn std::error::Error>> {
    let output = Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output()?;
    if !output.status.success() {
        return Err(format!("ps failed with status {}", output.status).into());
    }
    let text = String::from_utf8(output.stdout)?;
    Ok(text.trim().parse()?)
}

fn phys_footprint_bytes(pid: u32) -> Result<u64, Box<dyn std::error::Error>> {
    let output = Command::new("/usr/bin/footprint").args(["--pid", &pid.to_string(), "--format", "bytes"]).output()?;
    if !output.status.success() {
        return Err(format!("footprint failed with status {}", output.status).into());
    }
    let text = String::from_utf8(output.stdout)?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("phys_footprint:") {
            let bytes = value.split_whitespace().next().ok_or("missing phys_footprint value")?;
            return Ok(bytes.parse()?);
        }
    }
    Err("footprint output did not include phys_footprint".into())
}
