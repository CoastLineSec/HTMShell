use htm_runtime::run_package;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(package) = args.next() else {
        eprintln!("usage: htm-headless <shell-package-directory>");
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("htm-headless: expected exactly one package directory");
        std::process::exit(2);
    }

    let package = PathBuf::from(package);
    match run_package(&package) {
        Ok(run) => {
            let measurements = &run.measurements;
            println!(
                "HTMShell Gate A succeeded: {} phase(s), 1440x900 logical, scale 1.0, SDR/sRGB",
                run.artifacts.len()
            );
            println!(
                "timings: read={:.2}ms parse={:.2}ms resolve={:.2}ms paint={:.2}ms total={:.2}ms",
                measurements.package_read_ms,
                measurements.html_parse_ms,
                measurements.initial_resolve_ms,
                measurements.initial_paint_ms,
                measurements.total_ms
            );
            for artifact in &run.artifacts {
                let json = artifact
                    .diagnostic_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "not written".into());
                let png = artifact
                    .png_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "not produced".into());
                println!(
                    "{}: nodes={} json={} png={}",
                    artifact.phase.filename(),
                    artifact.report.node_count,
                    json,
                    png
                );
            }
        }
        Err(error) => {
            eprintln!("htm-headless: {error}");
            std::process::exit(1);
        }
    }
}
