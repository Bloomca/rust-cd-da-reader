/// Representation of read speed requested by the `SET CD SPEED` (0xBB) command.
///
/// # Note
///
/// According to the MMC-3 specification, the requested speed doesn't necessarily
/// match the actual read speed. The drive may select the specified read speed
/// or any higher rate.
///
/// Therefore, this enum represents a requested speed, not a guaranteed actual
/// read speed. The actual behaviour is drive-dependent.
#[derive(Debug, Clone, Copy)]
pub enum ReadSpeed {
    /// Don't change the speed
    ///
    /// The speed depends on the OS, previous configuration, and other factors.
    Unchanged,

    /// Request the drive-selected/optimal speed.
    ///
    /// According to the MMC-3 specification, the drive can select its optimal
    /// speed when the `SET CD SPEED` command is executed with the read
    /// speed (KB/s) set to 0xFFFF.
    ///
    /// On macOS, this variant requests `SET CD SPEED` with 0xFFFF.
    ///
    /// On Linux, the read speed is selected by the `CDROM_SELECT_SPEED`
    /// ioctl with speed = 0. It requests automatic speed selection.
    Optimal,

    /// Use a custom speed with the specified multiplier.
    ///
    /// Although the CD-DA read speed should be requested in KB/s according to
    /// the specification, this variant uses a multiplier for simplicity.
    ///
    /// For example, `ReadSpeed::CustomMultiplier(1)` represents the nominal
    /// CD-DA 1x read rate (176.4 KB/s). The exact conversion is
    /// platform-dependent.
    ///
    /// Another example: `ReadSpeed::CustomMultiplier(10)` represents 10x speed.
    ///
    /// `ReadSpeed::CustomMultiplier(0)` is equivalent to `ReadSpeed::Optimal`.
    /// The value 0 is used as a sentinel.
    CustomMultiplier(u8),
}
