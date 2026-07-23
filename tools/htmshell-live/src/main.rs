use htm_shell_host::{
    LiveHostOptions, ManifestHostOptions, MultiSurfaceHostOptions, SurfaceHostSummary,
    ValidatedManifest, run_live_overlay, run_manifest_shell, run_multi_surface_shell,
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
    if first == "manifest" {
        return run_manifest(args.collect());
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
    println!(
        "preferred_scale={}/{}",
        summary.preferred_scale_numerator, summary.scale_denominator
    );
    println!(
        "fractional_viewport_active={}",
        summary.fractional_viewport_active
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

fn run_manifest(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let mut args = arguments.into_iter();
    let path = PathBuf::from(
        args.next()
            .ok_or("manifest mode requires a path to shell.json")?,
    );
    let mut validate_only = false;
    let mut exit_after_initial_frames = false;
    let mut exit_after_output_events = None;
    let mut exit_after_actions = None;
    let mut exit_after_scale_changes = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--validate-only" => validate_only = true,
            "--exit-after-initial-frames" => exit_after_initial_frames = true,
            "--exit-after-output-events" => {
                exit_after_output_events = Some(
                    args.next()
                        .ok_or("--exit-after-output-events requires a positive integer")?
                        .parse::<u64>()?,
                );
            }
            "--exit-after-actions" => {
                exit_after_actions = Some(
                    args.next()
                        .ok_or("--exit-after-actions requires a positive integer")?
                        .parse::<u64>()?,
                );
            }
            "--exit-after-scale-changes" => {
                exit_after_scale_changes = Some(
                    args.next()
                        .ok_or("--exit-after-scale-changes requires a positive integer")?
                        .parse::<u64>()?,
                );
            }
            _ => return Err(format!("unknown manifest argument: {argument}").into()),
        }
    }
    if exit_after_output_events == Some(0) {
        return Err("--exit-after-output-events must be positive".into());
    }
    if exit_after_actions == Some(0) {
        return Err("--exit-after-actions must be positive".into());
    }
    if exit_after_scale_changes == Some(0) {
        return Err("--exit-after-scale-changes must be positive".into());
    }
    let manifest = ValidatedManifest::load(&path)?;
    if validate_only {
        println!("manifest_result=valid");
        println!("manifest_id={}", manifest.manifest().id);
        println!("manifest_version={}", manifest.manifest().version);
        println!("manifest_parse_count={}", manifest.parse_count());
        println!("surface_templates={}", manifest.manifest().surfaces.len());
        for surface in &manifest.manifest().surfaces {
            println!(
                "surface={} kind={:?} document={} namespace={}",
                surface.id(),
                surface.kind(),
                surface.document().display(),
                surface.namespace()
            );
        }
        return Ok(());
    }
    let summary = run_manifest_shell(ManifestHostOptions {
        manifest,
        exit_after_initial_frames,
        exit_after_output_events,
        exit_after_actions,
        exit_after_scale_changes,
    })?;
    println!("manifest_live_result=success");
    println!("manifest_id={}", summary.manifest_id);
    println!("manifest_parse_count={}", summary.manifest_parse_count);
    println!("manifest_parse_us={}", summary.manifest_parse_us);
    println!("manifest_validation_us={}", summary.manifest_validation_us);
    println!("layer_shell_version={}", summary.layer_shell_version);
    println!("viewporter_advertised={}", summary.viewporter_advertised);
    println!(
        "fractional_scale_advertised={}",
        summary.fractional_scale_advertised
    );
    println!("output_generations={}", summary.output_generations);
    println!("output_additions={}", summary.output_additions);
    println!("output_removals={}", summary.output_removals);
    println!(
        "unsupported_scale_outputs={}",
        summary.unsupported_scale_outputs
    );
    println!("active_outputs={}", summary.active_outputs.len());
    for output in &summary.active_outputs {
        let key = output
            .output_key
            .expect("live output summary has an identity");
        println!(
            "output={} generation={} label={}",
            key.global_name, key.generation, output.diagnostic_label
        );
        println!(
            "output_{}_ready_us={}",
            key.generation, output.output_ready_us
        );
        println!(
            "output_{}_first_panel_frame_us={}",
            key.generation, output.first_panel_frame_us
        );
        println!(
            "output_{}_overlay_open={}",
            key.generation, output.overlay_open
        );
        if let Some(panel) = &output.panel {
            println!(
                "output_{}_panel_instance={} owner={}",
                key.generation, panel.instance_generation, panel.owner
            );
            println!(
                "output_{}_panel_parse_count={}",
                key.generation, panel.metrics.html_parse_count
            );
            println!(
                "output_{}_panel_scale={}/{} fractional={} logical={}x{} physical={}x{} scale_commit_us={} scale_callback_us={}",
                key.generation,
                panel.metrics.preferred_scale_numerator,
                panel.metrics.scale_denominator,
                panel.metrics.fractional_viewport_active,
                panel.metrics.logical_width,
                panel.metrics.logical_height,
                panel.metrics.buffer_width,
                panel.metrics.buffer_height,
                panel.metrics.last_scale_change_to_commit_us,
                panel.metrics.last_scale_change_to_frame_callback_us
            );
            println!(
                "output_{}_panel_frames={}",
                key.generation, panel.metrics.frames_committed
            );
            println!(
                "output_{}_panel_callbacks={}",
                key.generation, panel.metrics.frame_callbacks
            );
            println!(
                "output_{}_panel_releases={}",
                key.generation, panel.metrics.buffer_releases
            );
            println!(
                "output_{}_panel_allocations={}",
                key.generation, panel.metrics.buffer_allocations
            );
            println!(
                "output_{}_panel_mapped_peak={}",
                key.generation, panel.metrics.mapped_memory_peak
            );
            println!(
                "output_{}_panel_busy_skips={} actions={}",
                key.generation, panel.metrics.busy_buffer_skips, panel.metrics.action_count
            );
            println!(
                "output_{}_panel_pointer={}/{}/{}",
                key.generation,
                panel.metrics.pointer_enters,
                panel.metrics.pointer_motions,
                panel.metrics.pointer_buttons
            );
            print_built_in_metrics(&format!("output_{}_panel", key.generation), &panel.metrics);
        }
        if let Some(overlay) = &output.overlay {
            println!(
                "output_{}_overlay_instance={} owner={}",
                key.generation, overlay.instance_generation, overlay.owner
            );
            println!(
                "output_{}_overlay_parse_count={}",
                key.generation, overlay.metrics.html_parse_count
            );
            println!(
                "output_{}_overlay_scale={}/{} fractional={} logical={}x{} physical={}x{} scale_commit_us={} scale_callback_us={}",
                key.generation,
                overlay.metrics.preferred_scale_numerator,
                overlay.metrics.scale_denominator,
                overlay.metrics.fractional_viewport_active,
                overlay.metrics.logical_width,
                overlay.metrics.logical_height,
                overlay.metrics.buffer_width,
                overlay.metrics.buffer_height,
                overlay.metrics.last_scale_change_to_commit_us,
                overlay.metrics.last_scale_change_to_frame_callback_us
            );
            println!(
                "output_{}_overlay_frames={}",
                key.generation, overlay.metrics.frames_committed
            );
            println!(
                "output_{}_overlay_callbacks={}",
                key.generation, overlay.metrics.frame_callbacks
            );
            println!(
                "output_{}_overlay_releases={}",
                key.generation, overlay.metrics.buffer_releases
            );
            println!(
                "output_{}_overlay_allocations={}",
                key.generation, overlay.metrics.buffer_allocations
            );
            println!(
                "output_{}_overlay_mapped_peak={}",
                key.generation, overlay.metrics.mapped_memory_peak
            );
            println!(
                "output_{}_overlay_busy_skips={} actions={}",
                key.generation, overlay.metrics.busy_buffer_skips, overlay.metrics.action_count
            );
            println!(
                "output_{}_overlay_pointer={}/{}/{}",
                key.generation,
                overlay.metrics.pointer_enters,
                overlay.metrics.pointer_motions,
                overlay.metrics.pointer_buttons
            );
            print_built_in_metrics(
                &format!("output_{}_overlay", key.generation),
                &overlay.metrics,
            );
        }
    }
    println!("peak_output_instances={}", summary.peak_output_instances);
    println!("peak_runtime_documents={}", summary.peak_runtime_documents);
    println!(
        "combined_mapped_memory_peak={}",
        summary.combined_mapped_memory_peak
    );
    println!("aggregate_shm_limit={}", summary.aggregate_shm_limit);
    println!(
        "stale_callbacks_contained={}",
        summary.stale_callbacks_contained
    );
    println!(
        "stale_releases_contained={}",
        summary.stale_releases_contained
    );
    println!(
        "stale_scale_events_contained={}",
        summary.stale_scale_events_contained
    );
    println!(
        "first_output_instance_us={}",
        summary.first_output_instance_us
    );
    println!(
        "last_output_teardown_us={}",
        summary.last_output_teardown_us
    );
    println!("actions={}", summary.actions);
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
    println!(
        "{prefix}_buffer_size={}x{}",
        summary.buffer_width, summary.buffer_height
    );
    println!(
        "{prefix}_scale={}/{} fractional={}",
        summary.preferred_scale_numerator,
        summary.scale_denominator,
        summary.fractional_viewport_active
    );
    println!(
        "{prefix}_preferred_scale_changes={}",
        summary.preferred_scale_changes
    );
    println!(
        "{prefix}_scale_change_to_commit_us={}",
        summary.last_scale_change_to_commit_us
    );
    println!(
        "{prefix}_scale_change_to_frame_callback_us={}",
        summary.last_scale_change_to_frame_callback_us
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
    print_built_in_metrics(prefix, summary);
}

fn print_built_in_metrics(prefix: &str, summary: &SurfaceHostSummary) {
    println!(
        "{prefix}_registry_initialization_us={}",
        summary.registry_initialization_us
    );
    println!(
        "{prefix}_declaration_discovery_us={}",
        summary.declaration_discovery_us
    );
    println!(
        "{prefix}_registered_elements={} bindings={} registered_actions={} registry_scans={}",
        summary.registered_element_count,
        summary.binding_count,
        summary.registered_action_count,
        summary.registry_scan_count
    );
    println!(
        "{prefix}_suppressed_binding_updates={}",
        summary.suppressed_binding_updates
    );
    println!(
        "{prefix}_component_latency_us=release_to_dispatch:{} dispatch_to_mutation:{} mutation_to_commit:{} mutation_to_callback:{}",
        summary.last_pointer_release_to_action_dispatch_us,
        summary.last_action_dispatch_to_state_mutation_us,
        summary.last_state_mutation_to_commit_us,
        summary.last_state_mutation_to_frame_callback_us,
    );
}
