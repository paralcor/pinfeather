//! 用户手动切换目标的只读诊断入口，仅 test-fixtures 构建。

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq, Eq)]
struct Args {
    label: String,
    seconds: u64,
    interval_ms: u64,
    delay_seconds: u64,
    output_name: Option<String>,
}

fn label_valid(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn parse_args(args: &[String]) -> Result<Args, &'static str> {
    let mut parsed = Args { label: String::new(), seconds: 12, interval_ms: 100,
        delay_seconds: 5, output_name: None };
    let mut seen = std::collections::HashSet::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].as_str();
        if !seen.insert(key) { return Err("duplicate argument"); }
        if !matches!(key, "--label" | "--seconds" | "--interval-ms" | "--delay-seconds" | "--output-name") {
            return Err("unknown argument");
        }
        let value = args.get(index + 1).ok_or("missing argument value")?;
        match key {
            "--label" => {
                if !label_valid(value, 32) { return Err("label must be 1-32 ASCII letters/digits/hyphens/underscores"); }
                parsed.label = value.clone();
            }
            "--output-name" => {
                if !value.strip_suffix(".jsonl").is_some_and(|stem| label_valid(stem, 48)) {
                    return Err("output-name must be a plain ASCII name ending in .jsonl");
                }
                parsed.output_name = Some(value.clone());
            }
            _ => {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("duration and interval must be unsigned decimal integers");
                }
                let number = value.parse::<u64>().map_err(|_| "integer out of range")?;
                match key {
                    "--seconds" if (1..=180).contains(&number) => parsed.seconds = number,
                    "--interval-ms" if (50..=1000).contains(&number) => parsed.interval_ms = number,
                    "--delay-seconds" if number <= 10 => parsed.delay_seconds = number,
                    _ => return Err("seconds 1-180; interval-ms 50-1000; delay-seconds 0-10"),
                }
            }
        }
        index += 2;
    }
    if parsed.label.is_empty() { return Err("--label is required"); }
    Ok(parsed)
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT")
        .ok_or("set PINFEATHER_TEST_DATA_ROOT to this run's isolated archive directory")?);
    if !root.is_absolute() || !root.is_dir() {
        return Err("PINFEATHER_TEST_DATA_ROOT must be an existing absolute directory".into());
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let name = args.output_name.unwrap_or_else(|| format!("{}-{stamp}.jsonl", args.label));
    let path = root.join(name);
    // 不覆盖既有证据，不使用默认产品目录。
    let mut output = std::fs::File::options().write(true).create_new(true).open(&path)?;
    eprintln!("Read-only target probe: label={}, output={}", args.label, path.display());
    eprintln!("Switch to the intended control yourself within {} seconds; sample for {} seconds. No input or clipboard access.",
        args.delay_seconds, args.seconds);
    std::thread::sleep(Duration::from_secs(args.delay_seconds));
    let start = Instant::now();
    let end = start + Duration::from_secs(args.seconds);
    let interval = Duration::from_millis(args.interval_ms);
    let mut samples = 0;
    while Instant::now() < end {
        let sampled_at = Instant::now();
        let observation = pinfeather_lib::platform::windows::observe_foreground();
        let row = serde_json::json!({
            "schema_version": 1,
            "label": args.label,
            "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().to_string(),
            "elapsed_ms": start.elapsed().as_millis() as u64,
            "target": observation,
        });
        serde_json::to_writer(&mut output, &row)?;
        output.write_all(b"\n")?;
        output.flush()?;
        samples += 1;
        let next = (sampled_at + interval).min(end);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    eprintln!("Completed {samples} observations. A label describes the planned scenario, not proof of the user's IME state.");
    Ok(())
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw == ["--help"] {
        println!("target_probe --label <scenario> [--seconds 12] [--interval-ms 100] [--delay-seconds 5] [--output-name <name.jsonl>]");
        println!("Requires PINFEATHER_TEST_DATA_ROOT. Read-only; never changes focus, input, or clipboard. Files are created exclusively.");
        return;
    }
    let result = parse_args(&raw).map_err(|e| e.into()).and_then(run);
    if let Err(error) = result {
        eprintln!("target_probe: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, &'static str> {
        parse_args(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn explicit_label_and_bounded_defaults() {
        let args = parse(&["--label", "edge-composing"]).unwrap();
        assert_eq!(args.seconds, 12);
        assert_eq!(args.interval_ms, 100);
        assert_eq!(args.delay_seconds, 5);
        assert!(args.output_name.is_none());
    }

    #[test]
    fn invalid_arguments_are_rejected_before_file_creation() {
        for args in [vec![], vec!["--label"], vec!["--label", "x", "--shell", "cmd"],
            vec!["--label", "x", "--label", "y"], vec!["--label", "x", "--seconds", "181"],
            vec!["--label", "x", "--seconds", "0"], vec!["--label", "x", "--interval-ms", "49"],
            vec!["--label", "x", "--delay-seconds", "11"], vec!["--label", "private text"],
            vec!["--label", "x", "--output-name", "../escape.jsonl"],
            vec!["--label", "x", "--output-name", "C:\\escape.jsonl"]] {
            assert!(parse(&args).is_err(), "must reject {args:?}");
        }
    }
}
