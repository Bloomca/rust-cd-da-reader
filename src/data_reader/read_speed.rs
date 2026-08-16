/// Representation of read speed that requested by SET CD SPEED (0xBB) command
#[derive(Debug, Clone, Copy)]
pub enum ReadSpeed {
    /// Don't change the speed
    /// The speed depends to the OS, previous configuration, and others
    Unchanged,

    /// Request to use the optimal speed
    /// By MMC-3 specification, It can be set to the optimal speed of the drive
    /// when it executes SET CD SPEED command with read speed (KB/s) = 0xFFFF.
    /// On the Linux, the read speed will selected by the CDROM_SELECT_SPEED
    /// ioctl with speed = 0. It'll set the speed automatically and highest
    /// speed that supported by the drive.
    Optimal,

    /// Use the custom speed with specified multiplier.
    ///
    /// Although CD-DA reading speed should be requested by KB/s unit by the
    /// specification, this constant will use multiplier for simple.
    /// For example, `ReadSpeed::CustomMultiplier(10)` is a representation
    /// of 10x speed.
    CustomMultiplier(u8),
}
