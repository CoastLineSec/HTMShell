use htm_shell_host::{LiveHostOptions, run_live_overlay};
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let package = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "examples/live-overlay".into()),
    );
    let mut exit_after_frames = None;
    let mut exit_after_click = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--exit-after-frames" => {
                let value = args
                    .next()
                    .ok_or("--exit-after-frames requires a positive integer")?;
                exit_after_frames = Some(value.parse::<u64>()?);
            }
            "--exit-after-click" => exit_after_click = true,
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    if exit_after_frames == Some(0) {
        return Err("--exit-after-frames must be positive".into());
    }

    let summary = run_live_overlay(LiveHostOptions {
        package,
        exit_after_frames,
        exit_after_click,
    })?;
    println!("live_result=success");
    println!("layer_shell_version={}", summary.layer_shell_version);
    println!(
        "configured_logical_size={}x{}",
        summary.logical_width, summary.logical_height
    );
    println!(
        "configured_buffer_size={}x{}",
        summary.buffer_width, summary.buffer_height
    );
    println!("output_scale={}", summary.output_scale);
    println!("viewporter_advertised={}", summary.viewporter_advertised);
    println!(
        "fractional_scale_advertised={}",
        summary.fractional_scale_advertised
    );
    println!("html_parse_count={}", summary.html_parse_count);
    println!("frames_committed={}", summary.frames_committed);
    println!("full_damage_commits={}", summary.full_damage_commits);
    println!("partial_damage_commits={}", summary.partial_damage_commits);
    println!("frame_callbacks={}", summary.frame_callbacks);
    println!("buffer_releases={}", summary.buffer_releases);
    println!("pointer_enters={}", summary.pointer_enters);
    println!("pointer_motions={}", summary.pointer_motions);
    println!("pointer_buttons={}", summary.pointer_buttons);
    println!("click_mutations={}", summary.click_mutations);
    println!("buffers_allocated={}", summary.buffers_allocated);
    println!("buffer_reallocations={}", summary.buffer_reallocations);
    println!("frames_skipped_busy={}", summary.frames_skipped_busy);
    println!("maximum_mapped_bytes={}", summary.maximum_mapped_bytes);
    println!("wayland_connection_us={}", summary.wayland_connection_us);
    println!("first_configure_us={}", summary.first_configure_us);
    println!("first_commit_us={}", summary.first_commit_us);
    println!(
        "first_frame_callback_us={}",
        summary.first_frame_callback_us
    );
    println!("package_read_us={}", summary.package_read_us);
    println!("html_parse_us={}", summary.html_parse_us);
    println!("initial_resolve_us={}", summary.initial_resolve_us);
    println!("last_resolve_us={}", summary.last_resolve_us);
    println!("last_render_us={}", summary.last_render_us);
    println!(
        "last_pixel_conversion_us={}",
        summary.last_pixel_conversion_us
    );
    Ok(())
}
