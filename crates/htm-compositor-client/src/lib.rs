//! Experimental client bindings for the compositor-neutral HTMShell contract.
//!
//! These generated bindings validate Gate B.0. They are not a stable public API.

#![forbid(unsafe_code)]

pub mod protocol {
    pub mod htm_shell_v1 {
        use wayland_client;
        use wayland_client::protocol::*;

        pub mod __interfaces {
            use wayland_client::protocol::__interfaces::*;

            wayland_scanner::generate_interfaces!("../../protocols/htm-shell-v1.xml");
        }
        use self::__interfaces::*;

        wayland_scanner::generate_client_code!("../../protocols/htm-shell-v1.xml");
    }
}

#[cfg(test)]
mod tests {
    const XML: &str = include_str!("../../../protocols/htm-shell-v1.xml");

    #[test]
    fn protocol_is_compositor_neutral() {
        for forbidden in ["hypr", "sway", "wlroots", "niri", "render_stage", "z_index"] {
            assert!(
                !XML.to_ascii_lowercase().contains(forbidden),
                "protocol contains compositor coupling: {forbidden}"
            );
        }
    }

    #[test]
    fn protocol_does_not_duplicate_surface_transport() {
        for forbidden_interface in ["htm_shell_buffer", "htm_shell_pointer", "htm_shell_output"] {
            assert!(!XML.contains(forbidden_interface));
        }
        assert!(XML.contains("interface=\"wl_surface\""));
        assert!(XML.contains("interface=\"wl_output\""));
    }
}
