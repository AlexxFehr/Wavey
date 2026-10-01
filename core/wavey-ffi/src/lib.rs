//! Swift-facing API of the Wavey core, exported with UniFFI.

uniffi::setup_scaffolding!();

#[uniffi::export]
pub fn core_version() -> String {
    wavey_geom::CORE_VERSION.to_string()
}
