//! Strict parsing and rendering of GitHub's contribution counts, without ESP-IDF.
use crate::framebuffer::{rgb, Framebuffer};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Snapshot {
    pub login: String,
    pub started_at: String,
    pub ended_at: String,
    pub commits: u64,
    pub pull_requests: u64,
    pub contributions: u64,
    pub weeks: Vec<[Option<u64>; 7]>,
}

impl Snapshot {
    pub fn parse(body: &[u8]) -> Result<Self, &'static str> {
        let value: Value = serde_json::from_slice(body).map_err(|_| "invalid_github_json")?;
        if value.get("errors").is_some() {
            return Err("github_graphql_error_check_token_permissions");
        }
        let user = &value["data"]["user"];
        let data = &user["contributionsCollection"];
        let calendar = &data["contributionCalendar"];
        let string = |value: &Value| {
            value
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 64)
                .map(str::to_owned)
                .ok_or("invalid_github_string")
        };
        let count = |value: &Value| {
            value
                .as_u64()
                .filter(|v| *v <= 100_000_000)
                .ok_or("invalid_github_count")
        };
        let raw_weeks = calendar["weeks"]
            .as_array()
            .ok_or("missing_github_calendar")?;
        if raw_weeks.is_empty() || raw_weeks.len() > 54 {
            return Err("invalid_github_calendar_length");
        }
        let mut weeks = Vec::new();
        for raw_week in raw_weeks {
            let mut week = [None; 7];
            let days = raw_week["contributionDays"]
                .as_array()
                .ok_or("invalid_github_week")?;
            if days.is_empty() || days.len() > 7 {
                return Err("invalid_github_week");
            }
            for day in days {
                let weekday = day["weekday"]
                    .as_u64()
                    .filter(|v| *v < 7)
                    .ok_or("invalid_github_weekday")? as usize;
                if week[weekday].is_some() {
                    return Err("duplicate_github_day");
                }
                week[weekday] = Some(count(&day["contributionCount"])?);
            }
            weeks.push(week);
        }
        let started_at = string(&data["startedAt"])?;
        let ended_at = string(&data["endedAt"])?;
        if !started_at.is_ascii()
            || !ended_at.is_ascii()
            || started_at.len() < 10
            || ended_at.len() < 10
        {
            return Err("invalid_github_dates");
        }
        Ok(Self {
            login: string(&user["login"])?,
            started_at,
            ended_at,
            commits: count(&data["totalCommitContributions"])?,
            pull_requests: count(&data["totalPullRequestContributions"])?,
            contributions: count(&calendar["totalContributions"])?,
            weeks,
        })
    }

    pub fn json(&self) -> Value {
        json!({"login": self.login, "started_at": self.started_at, "ended_at": self.ended_at,
            "commits": self.commits, "pull_requests": self.pull_requests, "contributions": self.contributions})
    }

    pub fn frame(&self, age_minutes: u64, stale: bool) -> Vec<u8> {
        let mut frame = Framebuffer::new(rgb(13, 17, 23));
        let white = rgb(230, 237, 243);
        let muted = rgb(139, 148, 158);
        let green = rgb(63, 185, 80);
        frame.rect(0, 0, 240, 5, green);
        frame.text(14, 20, "GITHUB", 3, white);
        frame.text(14, 54, &format!("@{}", self.login), 2, green);
        frame.text(14, 90, "CONTRIBUTIONS / YEAR", 1, muted);
        let total = self.contributions.to_string();
        frame.text(14, 105, &total, if total.len() <= 7 { 4 } else { 3 }, white);
        frame.rect(14, 147, 212, 1, rgb(48, 54, 61));
        frame.text(14, 162, "COMMITS", 1, muted);
        frame.text(130, 162, "PULL REQUESTS", 1, muted);
        frame.text(
            14,
            178,
            &self.commits.to_string(),
            if self.commits < 100_000 { 3 } else { 2 },
            white,
        );
        frame.text(
            130,
            178,
            &self.pull_requests.to_string(),
            if self.pull_requests < 100_000 { 3 } else { 2 },
            white,
        );
        frame.text(14, 219, "DAILY ACTIVITY", 1, muted);
        let colors = [
            rgb(22, 27, 34),
            rgb(14, 68, 41),
            rgb(0, 109, 50),
            rgb(38, 166, 65),
            rgb(57, 211, 83),
        ];
        for (x, week) in self.weeks.iter().enumerate() {
            for (y, count) in week.iter().enumerate() {
                if let Some(count) = count {
                    let level = match count {
                        0 => 0,
                        1..=3 => 1,
                        4..=9 => 2,
                        10..=19 => 3,
                        _ => 4,
                    };
                    frame.rect(12 + x * 4, 234 + y * 4, 3, 3, colors[level]);
                }
            }
        }
        frame.text(
            14,
            278,
            &format!("{} / {}", &self.started_at[..10], &self.ended_at[..10]),
            1,
            muted,
        );
        frame.text(14, 298, &format!("UPDATED {age_minutes} MIN AGO"), 1, muted);
        if stale {
            frame.text(14, 309, "CACHED / RETRYING", 1, rgb(240, 175, 65));
        }
        frame.0
    }
}

pub fn waiting_frame(configured: bool) -> Vec<u8> {
    let mut frame = Framebuffer::new(rgb(13, 17, 23));
    frame.text(16, 24, "GITHUB", 3, rgb(230, 237, 243));
    frame.text(
        16,
        116,
        if configured {
            "CONNECTING..."
        } else {
            "TOKEN NEEDED"
        },
        2,
        rgb(63, 185, 80),
    );
    frame.text(
        16,
        158,
        if configured {
            "WAITING FOR WI-FI"
        } else {
            "RUN GITHUB-SETUP"
        },
        1,
        rgb(139, 148, 158),
    );
    frame.text(
        16,
        177,
        if configured {
            "AND CLOCK SYNC"
        } else {
            "ON YOUR COMPUTER"
        },
        1,
        rgb(139, 148, 158),
    );
    frame.0
}
