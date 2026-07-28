use htm_runtime::{run_incremental_experiment, run_package};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        usage();
        std::process::exit(2);
    };
    if first == "mutate" {
        let Some(package) = args.next() else {
            usage();
            std::process::exit(2);
        };
        if args.next().is_some() {
            eprintln!("htm-headless: mutate expects exactly one package directory");
            std::process::exit(2);
        }
        run_mutation(PathBuf::from(package));
    } else {
        if args.next().is_some() {
            eprintln!("htm-headless: expected exactly one package directory");
            std::process::exit(2);
        }
        run_headless(PathBuf::from(first));
    }
}

fn usage() {
    eprintln!("usage: htm-headless <shell-package-directory>");
    eprintln!("       htm-headless mutate <shell-package-directory>");
}

fn run_headless(package: PathBuf) {
    match run_package(&package) {
        Ok(run) => {
            let measurements = &run.measurements;
            println!(
                "HTMShell headless run succeeded: {} phase(s), 1440x900 logical, scale 1.0, SDR/sRGB",
                run.artifacts.len()
            );
            println!(
                "package_snapshot={} package_id={} packages={}",
                run.package_snapshot.generation().get(),
                run.package_snapshot.root_package().id(),
                run.package_snapshot.packages().len()
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

fn run_mutation(package: PathBuf) {
    match run_incremental_experiment(&package) {
        Ok(run) => {
            println!(
                "HTMShell headless mutation run completed: parsed={} document-retained={} phases={} total={:.2}ms",
                run.document_parse_count,
                run.document_identity_preserved,
                run.artifacts.len(),
                run.total_ms
            );
            println!(
                "package_snapshot={} package_id={} packages={}",
                run.package_snapshot.generation().get(),
                run.package_snapshot.root_package().id(),
                run.package_snapshot.packages().len()
            );
            for artifact in &run.artifacts {
                let summary = artifact
                    .diff_from_previous
                    .as_ref()
                    .map(|diff| {
                        format!(
                            "retained={} created={} removed={} changed={}",
                            diff.summary.retained_unchanged,
                            diff.summary.created,
                            diff.summary.removed,
                            diff.summary.changed
                        )
                    })
                    .unwrap_or_else(|| "initial snapshot".into());
                println!(
                    "{}: nodes={} {}",
                    artifact.phase.filename(),
                    artifact.snapshot.node_count,
                    summary
                );
            }
            for baseline in &run.scale_baselines {
                println!(
                    "scale~{}: nodes={}->{} parse={:.2}ms resolve={:.2}ms paint={:.2}ms rss={}KiB",
                    baseline.requested_nodes,
                    baseline.exact_initial_nodes,
                    baseline.exact_final_nodes,
                    baseline.parse_ms,
                    baseline.initial_resolve_ms,
                    baseline.full_paint_ms,
                    baseline
                        .process_rss_kib
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unavailable".into())
                );
            }
            println!("paint model: every accepted painted phase rebuilt the full AnyRender scene");
            println!("artifacts: {}", package.join("output/mutation").display());
        }
        Err(error) => {
            eprintln!("htm-headless mutate: {error}");
            std::process::exit(1);
        }
    }
}
