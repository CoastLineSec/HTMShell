use htm_shell_host::{
    LiveHostOptions, MultiSurfaceHostOptions, SurfaceHostSummary, run_live_overlay,
    run_multi_surface_shell,
};
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .unwrap_or_else(|| "examples/live-overlay".into());
    if first == "two-surface" {
        return run_two_surface(args.collect());
    }
    let package = PathBuf::from(first);
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

fn run_two_surface(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let mut args = arguments.into_iter();
    let package = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "examples/two-surface-shell".into()),
    );
    let mut panel_height = 52;
    let mut automatic_overlay_cycles = 0;
    let mut exit_after_automatic_cycles = false;
    let mut exit_after_overlay_close = false;
    let mut open_overlay_on_start = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--panel-height" => {
                panel_height = args
                    .next()
                    .ok_or("--panel-height requires a positive integer")?
                    .parse()?;
            }
            "--automatic-overlay-cycles" => {
                automatic_overlay_cycles = args
                    .next()
                    .ok_or("--automatic-overlay-cycles requires a positive integer")?
                    .parse()?;
            }
            "--exit-after-automatic-cycles" => exit_after_automatic_cycles = true,
            "--exit-after-overlay-close" => exit_after_overlay_close = true,
            "--open-overlay-on-start" => open_overlay_on_start = true,
            _ => return Err(format!("unknown two-surface argument: {argument}").into()),
        }
    }
    if panel_height == 0 {
        return Err("--panel-height must be positive".into());
    }
    if exit_after_automatic_cycles && automatic_overlay_cycles == 0 {
        return Err("--exit-after-automatic-cycles requires automatic cycles".into());
    }
    if open_overlay_on_start && automatic_overlay_cycles > 0 {
        return Err("--open-overlay-on-start cannot be combined with automatic cycles".into());
    }
    let summary = run_multi_surface_shell(MultiSurfaceHostOptions {
        package,
        panel_height,
        automatic_overlay_cycles,
        exit_after_automatic_cycles,
        exit_after_overlay_close,
        open_overlay_on_start,
    })?;
    println!("multi_surface_result=success");
    println!("layer_shell_version={}", summary.layer_shell_version);
    println!("output_scale={}", summary.output_scale);
    println!("viewporter_advertised={}", summary.viewporter_advertised);
    println!(
        "fractional_scale_advertised={}",
        summary.fractional_scale_advertised
    );
    print_surface("panel", &summary.panel);
    print_surface("overlay", &summary.overlay);
    println!("overlay_open_count={}", summary.overlay_open_count);
    println!("overlay_close_count={}", summary.overlay_close_count);
    println!(
        "overlay_activation_count={}",
        summary.overlay_activation_count
    );
    println!(
        "panel_click_to_overlay_frame_us={}",
        summary.panel_click_to_overlay_frame_us
    );
    println!(
        "overlay_close_to_unmap_us={}",
        summary.overlay_close_to_unmap_us
    );
    println!(
        "combined_mapped_memory_peak={}",
        summary.combined_mapped_memory_peak
    );
    println!(
        "automatic_cycles_completed={}",
        summary.automatic_cycles_completed
    );
    println!("last_action={}", summary.last_action);
    Ok(())
}

fn print_surface(prefix: &str, summary: &SurfaceHostSummary) {
    println!(
        "{prefix}_logical_size={}x{}",
        summary.logical_width, summary.logical_height
    );
    println!("{prefix}_html_parse_count={}", summary.html_parse_count);
    println!("{prefix}_configure_count={}", summary.configure_count);
    println!("{prefix}_frames_committed={}", summary.frames_committed);
    println!("{prefix}_frame_callbacks={}", summary.frame_callbacks);
    println!("{prefix}_buffer_releases={}", summary.buffer_releases);
    println!("{prefix}_pointer_enters={}", summary.pointer_enters);
    println!("{prefix}_pointer_motions={}", summary.pointer_motions);
    println!("{prefix}_pointer_buttons={}", summary.pointer_buttons);
    println!("{prefix}_action_count={}", summary.action_count);
    println!("{prefix}_buffer_allocations={}", summary.buffer_allocations);
    println!(
        "{prefix}_buffer_reallocations={}",
        summary.buffer_reallocations
    );
    println!(
        "{prefix}_retired_buffer_peak={}",
        summary.retired_buffer_peak
    );
    println!("{prefix}_mapped_memory_peak={}", summary.mapped_memory_peak);
    println!("{prefix}_busy_buffer_skips={}", summary.busy_buffer_skips);
    println!("{prefix}_last_render_us={}", summary.last_render_us);
    println!(
        "{prefix}_last_pixel_conversion_us={}",
        summary.last_pixel_conversion_us
    );
}
