#[path = "../../../src/album_data.rs"]
mod album_data;
#[path = "../../../src/album_playback.rs"]
mod album_playback;
#[path = "../../../src/camera_config.rs"]
mod camera_config;
#[path = "../../../src/framebuffer.rs"]
mod framebuffer;
#[path = "../../../src/github_data.rs"]
mod github_data;
#[path = "../../../src/image_layout.rs"]
mod image_layout;
#[path = "../../../src/photo_record.rs"]
mod photo_record;
#[path = "../../../src/protocol.rs"]
mod protocol;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 3 {
        return Err("Usage: xiao-host-check activity.json output.rgb565".into());
    }
    let snapshot = github_data::Snapshot::parse(&std::fs::read(&arguments[1])?)?;
    println!("{}", snapshot.json());
    std::fs::write(&arguments[2], snapshot.frame(0, false))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response() -> serde_json::Value {
        json!({"data":{"user":{"login":"sample","contributionsCollection":{
            "startedAt":"2025-09-09T00:00:00Z","endedAt":"2026-09-09T00:00:00Z",
            "totalCommitContributions":12,"totalPullRequestContributions":3,
            "contributionCalendar":{"totalContributions":17,"weeks":[{"contributionDays":[
                {"weekday":2,"contributionCount":7},{"weekday":3,"contributionCount":10}
            ]}]}
        }}}})
    }

    #[test]
    fn preserve_contribution_semantics_and_missing_edge_days() {
        let snapshot =
            github_data::Snapshot::parse(&serde_json::to_vec(&response()).unwrap()).unwrap();
        assert_eq!(
            (
                snapshot.commits,
                snapshot.pull_requests,
                snapshot.contributions
            ),
            (12, 3, 17)
        );
        assert_eq!(
            snapshot.weeks[0],
            [None, None, Some(7), Some(10), None, None, None]
        );
        assert_eq!(snapshot.frame(0, false).len(), framebuffer::BYTE_LEN);
    }

    #[test]
    fn reject_partial_graphql_errors_and_missing_counters() {
        let mut value = response();
        value["errors"] = json!([{"message":"Resource not accessible"}]);
        assert!(github_data::Snapshot::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        value = response();
        value["data"]["user"]["contributionsCollection"]["totalCommitContributions"] =
            serde_json::Value::Null;
        assert!(github_data::Snapshot::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn reject_duplicate_weekdays_and_oversized_calendars() {
        let mut value = response();
        value["data"]["user"]["contributionsCollection"]["contributionCalendar"]["weeks"][0]
            ["contributionDays"][1]["weekday"] = json!(2);
        assert!(github_data::Snapshot::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut value = response();
        let week = value["data"]["user"]["contributionsCollection"]["contributionCalendar"]
            ["weeks"][0]
            .clone();
        value["data"]["user"]["contributionsCollection"]["contributionCalendar"]["weeks"] =
            json!(vec![week; 55]);
        assert!(github_data::Snapshot::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
