/// Representation of read speed requested by the SET CD SPEED (0xBB) command.
#[derive(Debug, Clone, Copy)]
pub enum ReadSpeed {
    /// Don't change the speed
    ///
    /// The speed depends to the OS, previous configuration, and other factors.
    Unchanged,

    /// Request the optimal speed.
    ///
    /// According to the MMC-3 specification, the drive can select its optimal
    /// speed  when the set SET CD SPEED command is executed with the read
    /// speed (KB/s) set to 0xFFFF.
    ///
    /// On the Linux, the read speed is selected by the CDROM_SELECT_SPEED
    /// ioctl with speed = 0. It selects the speed automatically up to the
    /// highest speed supported by the drive.
    Optimal,

    /// Use a custom speed with the specified multiplier.
    ///
    /// Although the CD-DA read speed should be requested in KB/s according to
    /// the specification, this variant uses a multiplier for simplicity. 
    /// Internally, it will be calculated as `176.4 KB/s * multiplier`.
    ///
    /// For example, `ReadSpeed::CustomMultiplier(10)` represents 10x speed. 
    CustomMultiplier(u8),
}
