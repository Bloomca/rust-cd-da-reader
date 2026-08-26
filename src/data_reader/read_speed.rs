/// A platform-independent read-speed request for an optical drive.
///
/// The requested speed is a preference, not a guarantee. Under MMC, a drive may
/// select the requested rate or a higher supported rate; the OS and drive
/// firmware may further adjust or reject the request.
///
/// A successful speed change may remain active for subsequent reads. This crate
/// does not restore the previous setting, although the OS, firmware, or other
/// software may change it later.
#[derive(Debug, Clone, Copy)]
pub enum ReadSpeed {
    /// Do not issue a speed-change request.
    ///
    /// This is the default in [`ReadOptions`](crate::ReadOptions). The drive
    /// retains whatever speed was previously selected by the OS, firmware, or
    /// another application.
    Unchanged,

    /// Ask the platform and drive to select an automatic or optimal read speed.
    ///
    /// On macOS and Windows, this uses the `0xFFFF` drive-selected/maximum-speed
    /// sentinel associated with `SET CD SPEED` (`0xBB`). On Linux,
    /// `CDROM_SELECT_SPEED` is called with a speed of zero to request automatic
    /// selection.
    Optimal,

    /// Request a multiple of the nominal CD-DA 1× rate.
    ///
    /// One unit represents 176.4 kB/s, so `CustomMultiplier(1)` requests 1× and
    /// `CustomMultiplier(10)` requests 10×. macOS and Windows convert the
    /// multiplier to kB/s, while Linux passes it as a speed multiplier.
    ///
    /// `CustomMultiplier(0)` is equivalent to [`Optimal`](Self::Optimal).
    CustomMultiplier(u8),
}
