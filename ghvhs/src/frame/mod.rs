use serde::{Deserialize, Serialize};
use spotcheck::{members::Member, teams::Team};

use crate::config::Configuration;

/// Metadata for the frame timing so we can index frames more accurately and track with
/// audit logs if needed.
#[derive(Serialize, Deserialize)]
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
#[derive(Serialize, Deserialize)]
pub struct Frame {
    /// The organization admins
    organization_admins: Vec<Member>,
    /// The organization members
    organization_members: Vec<Member>,
    /// The organization teams
    organization_teams: Vec<Team>,
    /// Shutter data for the frame
    shutter_data: ShutterData,
}

impl Frame {
    pub fn generate(configuration: &Configuration) -> Result<Self, String> {
        // Record when the frame generation started
        let start = std::time::SystemTime::now();
        let bootstrap = configuration.into();

        // Fetch all organization admins
        let mut organization_admins =
            spotcheck::members::audits::run_organization_admin_audit(&bootstrap)?
                .into_iter()
                .collect::<Vec<_>>();
        organization_admins.sort();

        let mut organization_members =
            spotcheck::members::audits::run_total_member_audit(&bootstrap)?
                .into_iter()
                .collect::<Vec<_>>();
        organization_members.sort();

        let mut organization_teams = spotcheck::teams::audits::run_team_audit(bootstrap)?
            .into_iter()
            .collect::<Vec<_>>();
        organization_teams.sort();

        // Record when the frame generation ended
        let end = std::time::SystemTime::now();

        // Calcuate the shutter speed
        let shutter_speed = end.duration_since(start).unwrap().as_secs();
        let open = start
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let close = end.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        Ok(Frame {
            organization_admins,
            organization_members,
            organization_teams,
            shutter_data: ShutterData {
                shutter_speed,
                open,
                close,
            },
        })
    }
}
