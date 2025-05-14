use spotcheck::{members::Member, Bootstrap};


/// Metadata for the frame timing so we can index frames more accurately and track with
/// audit logs if needed.
pub struct ShutterData {
    /// Due to pulling so much data, it takes a long time to generate a frame leading
    /// to the possibility that things changed while we were pulling the data. The shutter
    /// speed is the time it took to generate the frame in seconds.
    shutter_speed: u64,
    /// The time the frame generation started
    open: u64,
    /// The time the frame generation ended
    close: u64,
}

// A frame is the complete collection of data that represents
// a full view of the GitHub organization's permission at that moment
pub struct Frame {
    /// The organization admins
    organization_admins: Vec<Member>,
    /// Shutter data for the frame
    shutter_data: ShutterData,
}

impl Frame {
    pub fn generate(bootstrap: Bootstrap) -> Result<Self, String> {
        // Record when the frame generation started
        let start = std::time::SystemTime::now();
        
        // Fetch all organization admins
        let mut organization_admins = spotcheck::members::audits::run_organization_admin_audit(&bootstrap)?.into_iter().collect::<Vec<_>>();
        organization_admins.sort();

        
        // Record when the frame generation ended
        let end = std::time::SystemTime::now();

        // Calcuate the shutter speed
        let shutter_speed = end.duration_since(start).unwrap().as_secs();
        let open = start.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let close = end.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        Ok(Frame {
            organization_admins,
            shutter_data: ShutterData {
                shutter_speed,
                open,
                close,
            },
        })
    }
}