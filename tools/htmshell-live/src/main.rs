use htm_shell_host::{
    LiveHostOptions, ManifestHostOptions, MultiSurfaceHostOptions, SurfaceHostSummary,
    ValidatedManifest, run_live_overlay, run_manifest_shell, run_multi_surface_shell,
    run_pipewire_graph_diagnostic_json,
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
    if first == "pipewire-graph" {
        return run_pipewire_graph(args.collect());
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
    #[cfg(feature = "gpu-renderer")]
    print_gpu_metrics("surface", &summary.gpu);
    Ok(())
}

fn run_pipewire_graph(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    if !arguments.is_empty() {
        return Err("pipewire-graph does not accept arguments".into());
    }
    println!("{}", run_pipewire_graph_diagnostic_json()?);
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
    let mut exit_after_clock_updates = None;
    let mut exit_after_battery_updates = None;
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
            "--exit-after-clock-updates" => {
                exit_after_clock_updates = Some(
                    args.next()
                        .ok_or("--exit-after-clock-updates requires a positive integer")?
                        .parse::<u64>()?,
                );
            }
            "--exit-after-battery-updates" => {
                exit_after_battery_updates = Some(
                    args.next()
                        .ok_or("--exit-after-battery-updates requires a positive integer")?
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
    if exit_after_clock_updates == Some(0) {
        return Err("--exit-after-clock-updates must be positive".into());
    }
    if exit_after_battery_updates == Some(0) {
        return Err("--exit-after-battery-updates must be positive".into());
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
        exit_after_clock_updates,
        exit_after_battery_updates,
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
    println!("clock_format={}", summary.clock.format);
    println!("clock_effective_zone={}", summary.clock.effective_zone);
    println!("clock_utc_fallbacks={}", summary.clock.utc_fallbacks);
    println!(
        "clock_initialization_us={}",
        summary.clock.initialization_us
    );
    println!("clock_last_sample_us={}", summary.clock.last_sample_us);
    println!("clock_last_timezone_us={}", summary.clock.last_timezone_us);
    println!("clock_last_format_us={}", summary.clock.last_format_us);
    println!("clock_last_deadline_us={}", summary.clock.last_deadline_us);
    println!(
        "clock_last_timer_arm_us={}",
        summary.clock.last_timer_arm_us
    );
    println!("clock_wakeups={}", summary.clock.wakeups);
    println!("clock_expirations={}", summary.clock.expirations);
    println!("clock_changed_values={}", summary.clock.changed_values);
    println!(
        "clock_unchanged_values_suppressed={}",
        summary.clock.unchanged_values_suppressed
    );
    println!(
        "clock_wall_clock_resets={}",
        summary.clock.wall_clock_resets
    );
    println!("clock_subscribers={}", summary.clock.subscribers);
    println!(
        "clock_maximum_subscribers={}",
        summary.clock.maximum_subscribers
    );
    println!(
        "clock_timer_descriptors={}",
        summary.clock.timer_descriptors
    );
    println!("clock_generation={}", summary.clock.generation);
    println!("clock_sequence={}", summary.clock.sequence);
    println!(
        "clock_sampled_unix_seconds={}",
        summary.clock.sampled_unix_seconds
    );
    println!(
        "clock_documents_visited={}",
        summary.clock.documents_visited
    );
    println!("clock_elements_mutated={}", summary.clock.elements_mutated);
    println!("clock_fanout_us={}", summary.clock.fanout_us);
    println!(
        "clock_panel_frames_scheduled={}",
        summary.clock.panel_frames_scheduled
    );
    println!(
        "clock_unrelated_frames_scheduled={}",
        summary.clock.unrelated_frames_scheduled
    );
    println!(
        "clock_closed_surface_frames_suppressed={}",
        summary.clock.closed_surface_frames_suppressed
    );
    println!(
        "clock_mutation_failures_contained={}",
        summary.clock.mutation_failures_contained
    );
    println!("clock_declarations={}", summary.clock.declarations);
    println!(
        "clock_enabled_declarations={}",
        summary.clock.enabled_declarations
    );
    println!(
        "clock_maximum_declarations={}",
        summary.clock.maximum_declarations
    );
    println!("clock_unique_formats={}", summary.clock.unique_formats);
    println!("clock_unique_zones={}", summary.clock.unique_zones);
    println!(
        "clock_unique_zone_conversions={}",
        summary.clock.unique_zone_conversions
    );
    println!(
        "clock_unique_format_operations={}",
        summary.clock.unique_format_operations
    );
    println!(
        "clock_cached_render_key_reuse={}",
        summary.clock.cached_render_key_reuse
    );
    println!(
        "clock_format_compilation_us={}",
        summary.clock.format_compilation_us
    );
    println!(
        "clock_timezone_lookup_us={}",
        summary.clock.timezone_lookup_us
    );
    println!(
        "clock_deadline_calculation_us={}",
        summary.clock.deadline_calculation_us
    );
    println!(
        "clock_changed_declarations={}",
        summary.clock.changed_declarations
    );
    println!(
        "clock_suppressed_declarations={}",
        summary.clock.suppressed_declarations
    );
    println!("battery_transport={}", summary.battery.transport);
    println!(
        "battery_lifecycle_state={}",
        summary.battery.lifecycle_state
    );
    println!("battery_subscribers={}", summary.battery.subscribers);
    println!("upower_subscribers={}", summary.battery.upower_subscribers);
    println!(
        "power_profile_subscribers={}",
        summary.battery.profile_subscribers
    );
    println!(
        "battery_maximum_subscribers={}",
        summary.battery.maximum_subscribers
    );
    println!(
        "battery_source_generation={}",
        summary.battery.source_generation
    );
    println!("battery_sequence={}", summary.battery.sequence);
    println!("battery_availability={}", summary.battery.availability);
    println!("upower_power_source={}", summary.battery.on_battery);
    println!(
        "battery_percentage={}",
        summary
            .battery
            .percentage
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".into())
    );
    println!("battery_charge_state={}", summary.battery.charge_state);
    println!("battery_warning={}", summary.battery.warning);
    println!("upower_device_count={}", summary.battery.device_count);
    println!("power_profile={}", summary.battery.profile);
    println!(
        "power_profile_available={}",
        summary.battery.profile_available
    );
    println!(
        "power_profile_performance_available={}",
        summary.battery.performance_available
    );
    println!("power_profile_degradation={}", summary.battery.degradation);
    println!("power_profile_hold_count={}", summary.battery.hold_count);
    println!(
        "power_profile_source_generation={}",
        summary.battery.profile_source_generation
    );
    println!(
        "power_profile_requests={}",
        summary.battery.profile_requests
    );
    println!(
        "power_profile_request_failures={}",
        summary.battery.profile_request_failures
    );
    println!(
        "battery_system_bus_connections={}",
        summary.battery.system_bus_connections
    );
    println!(
        "battery_connection_failures={}",
        summary.battery.connection_failures
    );
    println!(
        "battery_service_appearances={}",
        summary.battery.service_appearances
    );
    println!(
        "battery_service_disappearances={}",
        summary.battery.service_disappearances
    );
    println!(
        "battery_owner_replacements={}",
        summary.battery.owner_replacements
    );
    println!(
        "battery_property_signals={}",
        summary.battery.property_signals
    );
    println!(
        "battery_property_bursts={}",
        summary.battery.property_bursts
    );
    println!("battery_refreshes={}", summary.battery.refreshes);
    println!(
        "battery_refresh_failures={}",
        summary.battery.refresh_failures
    );
    println!(
        "battery_bus_disconnects={}",
        summary.battery.bus_disconnects
    );
    println!(
        "battery_reconnect_attempts={}",
        summary.battery.reconnect_attempts
    );
    println!(
        "battery_duplicate_snapshots_suppressed={}",
        summary.battery.duplicate_snapshots_suppressed
    );
    println!(
        "battery_changed_snapshots={}",
        summary.battery.changed_snapshots
    );
    println!(
        "battery_malformed_values={}",
        summary.battery.malformed_values
    );
    println!(
        "battery_messages_drained={}",
        summary.battery.messages_drained
    );
    println!(
        "battery_initial_connection_us={}",
        summary.battery.initial_connection_us
    );
    println!(
        "battery_last_owner_lookup_us={}",
        summary.battery.last_owner_lookup_us
    );
    println!(
        "battery_last_property_read_us={}",
        summary.battery.last_property_read_us
    );
    println!(
        "battery_last_enumeration_us={}",
        summary.battery.last_enumeration_us
    );
    println!(
        "battery_last_device_read_us={}",
        summary.battery.last_device_read_us
    );
    println!(
        "battery_last_profiles_read_us={}",
        summary.battery.last_profiles_read_us
    );
    println!(
        "battery_last_signal_to_refresh_us={}",
        summary.battery.last_signal_to_refresh_us
    );
    println!(
        "battery_last_reconnect_us={}",
        summary.battery.last_reconnect_us
    );
    println!(
        "battery_transport_descriptors={}",
        summary.battery.transport_descriptors
    );
    println!(
        "battery_maximum_transport_descriptors={}",
        summary.battery.maximum_transport_descriptors
    );
    println!(
        "battery_dbus_watch_count_peak={}",
        summary.battery.dbus_watch_count_peak
    );
    println!(
        "battery_match_rules_installed={}",
        summary.battery.match_rules_installed
    );
    println!(
        "battery_deadline_descriptors={}",
        summary.battery.deadline_descriptors
    );
    println!(
        "battery_explicit_worker_threads={}",
        summary.battery.explicit_worker_threads
    );
    println!(
        "battery_internal_threads={}",
        summary.battery.internal_threads
    );
    println!(
        "battery_documents_visited={}",
        summary.battery.documents_visited
    );
    println!(
        "battery_elements_mutated={}",
        summary.battery.elements_mutated
    );
    println!("battery_fanout_us={}", summary.battery.fanout_us);
    println!(
        "battery_frames_scheduled={}",
        summary.battery.frames_scheduled
    );
    println!(
        "battery_unrelated_frames_scheduled={}",
        summary.battery.unrelated_frames_scheduled
    );
    println!(
        "battery_closed_surface_frames_suppressed={}",
        summary.battery.closed_surface_frames_suppressed
    );
    println!(
        "battery_mutation_failures_contained={}",
        summary.battery.mutation_failures_contained
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
    let mut exit_after_peak_publications = None;
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
            "--exit-after-peak-publications" => {
                exit_after_peak_publications = Some(
                    args.next()
                        .ok_or("--exit-after-peak-publications requires a positive integer")?
                        .parse::<u64>()?,
                );
            }
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
    if exit_after_peak_publications == Some(0) {
        return Err("--exit-after-peak-publications must be positive".into());
    }
    let summary = run_multi_surface_shell(MultiSurfaceHostOptions {
        package,
        panel_height,
        automatic_overlay_cycles,
        exit_after_automatic_cycles,
        exit_after_overlay_close,
        open_overlay_on_start,
        exit_after_peak_publications,
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
    println!(
        "pipewire_peak_streams_active={}",
        summary.pipewire_peaks.active_streams
    );
    println!(
        "pipewire_peak_stream_starts={} stops={}",
        summary.pipewire_peaks.stream_starts, summary.pipewire_peaks.stream_stops
    );
    println!(
        "pipewire_peak_callbacks={} coalesced={}",
        summary.pipewire_peaks.process_callbacks, summary.pipewire_peaks.callbacks_coalesced
    );
    println!(
        "pipewire_peak_vectors_published={} duplicates={}",
        summary.pipewire_peaks.vectors_published,
        summary.pipewire_peaks.duplicate_vectors_suppressed
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
    println!("{prefix}_package_read_us={}", summary.package_read_us);
    println!("{prefix}_html_parse_us={}", summary.html_parse_us);
    println!(
        "{prefix}_initial_resource_resolve_us={}",
        summary.initial_resolve_us
    );
    println!("{prefix}_last_resolve_us={}", summary.last_resolve_us);
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
        "{prefix}_registered_elements={} bindings={} text_bindings={} token_bindings={} registered_actions={} clock_declarations={} repeat_declarations={} registry_scans={}",
        summary.registered_element_count,
        summary.binding_count,
        summary.text_binding_count,
        summary.token_binding_count,
        summary.registered_action_count,
        summary.clock_declaration_count,
        summary.repeat_declaration_count,
        summary.registry_scan_count
    );
    println!(
        "{prefix}_suppressed_binding_updates={}",
        summary.suppressed_binding_updates
    );
    println!(
        "{prefix}_token_updates=changed:{} suppressed:{} projection_us:{} attribute_mutation_us:{}",
        summary.changed_token_updates,
        summary.suppressed_token_updates,
        summary.last_state_projection_us,
        summary.last_attribute_mutation_us,
    );
    println!(
        "{prefix}_repeat_updates=insertions:{} removals:{} moves:{} properties:{} unchanged:{} clones:{} identity_reuses:{} items:{} cloned_nodes:{} reconciliation_us:{}",
        summary.repeat_insertions,
        summary.repeat_removals,
        summary.repeat_moves,
        summary.repeat_property_updates,
        summary.repeat_unchanged_items,
        summary.repeat_subtree_clones,
        summary.repeat_identity_reuses,
        summary.repeated_item_count,
        summary.cloned_node_count,
        summary.last_reconciliation_us,
    );
    println!(
        "{prefix}_channel_updates=activations:{} releases:{} insertions:{} removals:{} moves:{} layouts:{} values:{} clones:{} identity_reuses:{} duplicates:{} items:{}",
        summary.channel_source_activations,
        summary.channel_source_releases,
        summary.channel_insertions,
        summary.channel_removals,
        summary.channel_moves,
        summary.channel_layout_replacements,
        summary.channel_value_mutations,
        summary.contextual_subtree_clones,
        summary.retained_channel_identities,
        summary.duplicate_channel_suppressions,
        summary.contextual_item_count,
    );
    println!(
        "{prefix}_graph_updates=links:+{}/-{} state:{} relations:{} moves:{} groups:+{}/-{} members:+{}/-{} representatives:{} group_state:{} trackers:+{}/-{} peers:{} retained_links:{} retained_groups:{} retained_trackers:{} duplicates:{}",
        summary.link_insertions,
        summary.link_removals,
        summary.link_state_mutations,
        summary.link_relation_mutations,
        summary.link_moves,
        summary.group_insertions,
        summary.group_removals,
        summary.group_member_insertions,
        summary.group_member_removals,
        summary.representative_changes,
        summary.group_state_mutations,
        summary.node_tracker_insertions,
        summary.node_tracker_removals,
        summary.peer_relation_mutations,
        summary.retained_link_identities,
        summary.retained_group_identities,
        summary.retained_tracker_identities,
        summary.duplicate_graph_suppressions,
    );
    println!(
        "{prefix}_component_latency_us=release_to_dispatch:{} dispatch_to_mutation:{} mutation_to_commit:{} mutation_to_callback:{}",
        summary.last_pointer_release_to_action_dispatch_us,
        summary.last_action_dispatch_to_state_mutation_us,
        summary.last_state_mutation_to_commit_us,
        summary.last_state_mutation_to_frame_callback_us,
    );
    #[cfg(feature = "gpu-renderer")]
    print_gpu_metrics(prefix, &summary.gpu);
}

#[cfg(feature = "gpu-renderer")]
fn print_gpu_metrics(prefix: &str, summary: &htm_shell_host::GpuSurfaceHostSummary) {
    println!(
        "{prefix}_gpu=requested:{} success:{} state:{} format:{} present_mode:{} alpha_mode:{} configuration_generation:{}",
        summary.requested,
        summary.successful_gpu_frame,
        summary.presenter_state,
        summary.surface_format,
        summary.present_mode,
        summary.alpha_mode,
        summary.configuration_generation,
    );
    println!(
        "{prefix}_gpu_backend=adapter:{} api:{} device_type:{} driver:{} device_generation:{}",
        summary.adapter,
        summary.graphics_api,
        summary.device_type,
        summary.driver,
        summary.device_generation,
    );
    println!(
        "{prefix}_gpu_presenters=created:{} released:{} configurations:{} reconfigurations:{} target_recreations:{}",
        summary.presenter_creations,
        summary.presenter_releases,
        summary.configurations,
        summary.reconfigurations,
        summary.target_recreations,
    );
    println!(
        "{prefix}_gpu_frames=planned:{} rendered:{} submitted:{} presented:{} acquisitions:{} acquisition_failures:{} conversion_passes:{} full_target:{} cpu_fallbacks:{} shm:{}",
        summary.frames_planned,
        summary.frames_rendered,
        summary.frames_submitted,
        summary.frames_presented,
        summary.surface_acquisitions,
        summary.acquisition_failures,
        summary.conversion_passes,
        summary.full_target_renders,
        summary.cpu_fallbacks,
        summary.shm_frames,
    );
    println!(
        "{prefix}_gpu_callbacks=requested:{} completed:{} losses:{} timeouts:{} outdated:{} device_losses:{} closed_suppressions:{} duplicate_suppressions:{}",
        summary.frame_callbacks_requested,
        summary.frame_callbacks_completed,
        summary.surface_losses,
        summary.surface_timeouts,
        summary.surface_outdated,
        summary.device_losses,
        summary.closed_surface_suppressions,
        summary.duplicate_frame_suppressions,
    );
    println!(
        "{prefix}_gpu_resources=entries:{} bytes:{} uploads:{} cache_hits:{}",
        summary.resource_entries,
        summary.resource_bytes,
        summary.resource_uploads,
        summary.cache_hits,
    );
    if !summary.last_error.is_empty() {
        println!("{prefix}_gpu_last_error={}", summary.last_error);
    }
}
